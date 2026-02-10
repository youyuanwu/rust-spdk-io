# Runtime Architecture

## Design Goals

1. **Runtime-agnostic** - works with any single-threaded async executor
2. **User controls the runtime** - start SPDK thread, run your preferred local executor
3. **Async/await for I/O operations** - no manual callback management
4. **Thread-local I/O channels** - lock-free I/O submission
5. **Cooperative scheduling** - yield between SPDK polling and app logic
6. **Uses standard futures traits** - `Future`, `Stream`, `Sink` from `futures` crate

## Threading Model

**Note:** An `spdk_thread` is NOT an OS thread. It's a lightweight scheduling context 
(similar to a green thread or goroutine). It runs on whatever OS thread calls 
`spdk_thread_poll()` on it. Think of it as a task queue + poller state.

```
┌─────────────────────────────────────────────────────────────┐
│                     OS Thread N                              │
│  ┌─────────────────────────────────────────────────────────┐│
│  │         Any Local Async Executor (user's choice)        ││
│  │       (Tokio LocalSet, smol, async-executor, etc.)      ││
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  ││
│  │  │ App Future  │  │ App Future  │  │ SPDK Poller     │  ││
│  │  │   (task)    │  │   (task)    │  │   (task)        │  ││
│  │  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  ││
│  │         │                │                   │           ││
│  │         ▼                ▼                   ▼           ││
│  │  ┌───────────────────────────────────────────────────┐  ││
│  │  │              I/O Channel (thread-local)            │  ││
│  │  └───────────────────────────────────────────────────┘  ││
│  └─────────────────────────────────────────────────────────┘│
│                              │                               │
│                              ▼                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    SPDK Thread Context                   ││
│  │     (spdk_thread struct - message queue, pollers)        ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

Each OS thread that uses SPDK has:
- An **SPDK thread context** (`spdk_thread`) - a scheduling/message queue, not a real thread
- **User's choice of local executor** - any `!Send` future executor works
- A **poller task** that calls `spdk_thread_poll()` and yields to the executor

## Async Integration Pattern

SPDK uses callback-based async. We convert to Rust futures using `futures-channel`:

```rust
use futures_channel::oneshot;
use futures_util::future::FutureExt;

pub async fn bdev_read(
    desc: &BdevDesc,
    channel: &IoChannel,
    buf: &mut DmaBuf,
    offset: u64,
    len: u64,
) -> Result<(), SpdkError> {
    // Create oneshot channel for completion (runtime-agnostic)
    let (tx, rx) = oneshot::channel();
    
    // Submit I/O with callback that sends on channel
    unsafe {
        spdk_bdev_read(
            desc.as_ptr(),
            channel.as_ptr(),
            buf.as_mut_ptr(),
            offset,
            len,
            Some(completion_callback),
            tx.into_raw(),
        );
    }
    
    // Await completion (yields to executor, SPDK poller runs)
    rx.await.map_err(|_| SpdkError::Cancelled)?
}

extern "C" fn completion_callback(
    bdev_io: *mut spdk_bdev_io,
    success: bool,
    ctx: *mut c_void,
) {
    let tx = unsafe { Sender::from_raw(ctx) };
    let result = if success { Ok(()) } else { Err(SpdkError::IoError) };
    let _ = tx.send(result);
    unsafe { spdk_bdev_free_io(bdev_io) };
}
```

## SPDK Poller Integration

The SPDK poller runs as an async task that:
1. Calls `spdk_thread_poll()` to process SPDK work
2. Yields to allow other tasks to run
3. Repeats

```rust
use futures_util::future::yield_now;

/// Poller task that drives SPDK's internal event loop
/// Works with any async executor
pub async fn spdk_poller_task(thread: &SpdkThread) {
    loop {
        // Poll SPDK - this processes completions and runs pollers
        let work_done = thread.poll();
        
        if work_done == 0 {
            // No work done, yield to other tasks (runtime-agnostic)
            yield_now().await;
        }
        // If work was done, immediately poll again (busy loop)
    }
}
```

## Runtime Initialization

The user controls the runtime. `spdk-io` provides the SPDK thread and poller:

```rust
use spdk_io::{SpdkEnv, SpdkThread, poller_task};

fn main() {
    // Initialize SPDK environment (hugepages, PCI, etc.)
    let _env = SpdkEnv::builder()
        .name("my_app")
        .mem_size_mb(2048)
        .build()
        .expect("Failed to init SPDK");
    
    // Attach SPDK thread context to current OS thread
    let spdk_thread = SpdkThread::current("worker-0").expect("Failed to attach");
    
    // User chooses their runtime - here's Tokio example:
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        // Spawn the SPDK poller as a background task
        tokio::task::spawn_local(poller_task(&spdk_thread));
        
        // Pass thread handle to app
        run_app(&spdk_thread).await
    });
}
```

### With smol

```rust
use smol::LocalExecutor;

fn main() {
    let _env = SpdkEnv::builder().name("app").build().unwrap();
    let spdk_thread = SpdkThread::current("worker").unwrap();
    
    let ex = LocalExecutor::new();
    futures_lite::future::block_on(ex.run(async {
        ex.spawn(poller_task(&spdk_thread)).detach();
        run_app().await
    }));
}
```

### With async-executor

```rust
use async_executor::LocalExecutor;

fn main() {
    let _env = SpdkEnv::builder().name("app").build().unwrap();
    let spdk_thread = SpdkThread::current("worker").unwrap();
    
    let ex = LocalExecutor::new();
    futures_lite::future::block_on(ex.run(async {
        ex.spawn(poller_task(&spdk_thread)).detach();
        run_app().await
    }));
}
```
