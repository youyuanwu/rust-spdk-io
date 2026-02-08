# spdk-io Design Document

## Overview

`spdk-io` is a Rust library providing safe, ergonomic, async-first bindings to SPDK (Storage Performance Development Kit). The library enables Rust applications to leverage SPDK's high-performance user-space storage stack with native async/await syntax.

## Implementation Status

### Completed

| Component | Status | Notes |
|-----------|--------|-------|
| **spdk-io-sys crate** | ✅ | FFI bindings via bindgen |
| - pkg-config integration | ✅ | Static linking with `--whole-archive` |
| - bindgen generation | ✅ | Rust 2024 compatible, `wrap_unsafe_ops(true)` |
| - System deps handling | ✅ | Filters archive names, probes OpenSSL/ISA-L/uuid |
| **spdk-io crate** | 🟡 | Core types implemented |
| - `SpdkEnv` | ✅ | Environment guard with RAII cleanup |
| - `SpdkEnvBuilder` | ✅ | Full configuration: name, core_mask, mem_size, shm_id, no_pci, no_huge, main_core |
| - `Error` types | ✅ | Comprehensive error enum with thiserror |
| - Integration tests | ✅ | vdev mode (no hugepages required) |
| **CI/CD** | ✅ | GitHub Actions with SPDK deb package |

### In Progress

| Component | Status | Notes |
|-----------|--------|-------|
| `SpdkThread` | ⏳ | Thread context management |
| `IoChannel` | ⏳ | Thread-local I/O channels |
| SPDK poller task | ⏳ | Async executor integration |

### Planned

| Component | Notes |
|-----------|-------|
| `Bdev` / `BdevDesc` | Block device API with async read/write |
| `DmaBuf` | DMA-capable buffer allocation |
| `Blobstore` / `Blob` | Blobstore API |
| `NvmeController` | Direct NVMe access |
| Callback-to-future utilities | `oneshot` channel pattern |

### Build & Linking

The crate uses **static linking** with `--whole-archive` for SPDK/DPDK libraries:

```
┌─────────────────────────────────────────────────────────────┐
│  spdk-io-sys build.rs                                       │
├─────────────────────────────────────────────────────────────┤
│  1. pkg-config probes SPDK libs (statik=true)               │
│  2. Separates SPDK/DPDK libs from system libs               │
│  3. Emits --whole-archive for SPDK/DPDK (include all syms)  │
│  4. Links system libs normally (ssl, crypto, numa, etc.)    │
│  5. bindgen generates Rust bindings from wrapper.h          │
└─────────────────────────────────────────────────────────────┘
```

**Why static linking?** SPDK uses callback tables and static initializers that would be
dropped by the linker with `--as-needed`. Using `--whole-archive` ensures all symbols
are included in the final binary.

## Crate Structure

```
spdk-io/
├── spdk-io-sys/          # Low-level FFI bindings
│   ├── build.rs          # Bindgen + linking
│   ├── src/
│   │   └── lib.rs        # Generated bindings + manual additions
│   └── wrapper.h         # SPDK headers to bind
│
└── spdk-io/              # High-level async Rust API
    ├── src/
    │   ├── lib.rs
    │   ├── env.rs        # Environment initialization
    │   ├── thread.rs     # SPDK thread management
    │   ├── poller.rs     # SPDK poller (async task)
    │   ├── bdev.rs       # Block device API
    │   ├── blob.rs       # Blobstore API
    │   ├── nvme.rs       # NVMe driver API
    │   ├── channel.rs    # I/O channel management
    │   ├── dma.rs        # DMA buffer management
    │   ├── error.rs      # Error types
    │   └── complete.rs   # Callback-to-future utilities
    └── Cargo.toml
```

### spdk-io-sys

Low-level FFI bindings crate:

- **Generated via bindgen** from SPDK headers
- **Links to SPDK** static libraries via pkg-config or explicit paths
- **Exports raw types**: `spdk_bdev`, `spdk_blob`, `spdk_io_channel`, etc.
- **Exports raw functions**: `spdk_bdev_read()`, `spdk_blob_io_write()`, etc.
- **Minimal safe wrappers**: Only for ergonomics (e.g., `Default` impls)

### spdk-io

High-level async Rust crate:

- **Safe wrappers** around `spdk-io-sys` types
- **Async/await API** for all I/O operations
- **Runtime-agnostic** - works with any local executor (Tokio, async-std, smol, etc.)
- **RAII resource management** (Drop implementations)
- **Error handling** via `Result<T, SpdkError>`
- **Uses `futures` ecosystem** - `futures-util`, `futures-channel` for portability

## Runtime Architecture

### Design Goals

1. **Runtime-agnostic** - works with any single-threaded async executor
2. **User controls the runtime** - start SPDK thread, run your preferred local executor
3. **Async/await for I/O operations** - no manual callback management
4. **Thread-local I/O channels** - lock-free I/O submission
5. **Cooperative scheduling** - yield between SPDK polling and app logic
6. **Uses standard futures traits** - `Future`, `Stream`, `Sink` from `futures` crate

### Threading Model

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

### Async Integration Pattern

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

### SPDK Poller Integration

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

### Runtime Initialization

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
    
    // For testing without hugepages (vdev mode):
    // let _env = SpdkEnv::builder()
    //     .name("test")
    //     .no_pci(true)
    //     .no_huge(true)
    //     .mem_size_mb(64)
    //     .build()
    //     .expect("Failed to init SPDK");
    
    // Attach SPDK thread context to current OS thread (no new thread created)
    let spdk_thread = SpdkThread::current("worker-0").expect("Failed to attach SPDK thread");
    
    // User chooses their runtime - here's Tokio example:
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        // Spawn the SPDK poller as a background task
        tokio::task::spawn_local(poller_task(&spdk_thread));
        
        // Pass thread handle to app (explicit, no hidden state)
        run_app(&spdk_thread).await
    });
}

async fn run_app(thread: &SpdkThread) -> Result<(), SpdkError> {
    // Get bdev by name
    let bdev = Bdev::get_by_name("Nvme0n1").await?;
    
    // Open with read-write access
    let desc = bdev.open(true).await?;
    
    // Get I/O channel from thread handle (explicit, no globals)
    let channel = thread.get_io_channel(&desc)?;
    
    // Allocate DMA buffer
    let mut buf = DmaBuf::alloc(4096, 4096)?;
    
    // Async read
    desc.read(channel, &mut buf, 0, 4096).await?;
    
    Ok(())
}
```

### Alternative Runtime Examples

**With `smol`:**
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

**With `async-executor`:**
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

## Core Types

### Environment & Initialization

```rust
/// SPDK environment guard - initialized ONCE per process
/// 
/// Like DPDK's EAL, the SPDK environment can only be initialized once.
/// This struct acts as a guard: when dropped, SPDK is cleaned up but
/// CANNOT be re-initialized (DPDK limitation).
/// 
/// Typically held in main() for the lifetime of the program.
pub struct SpdkEnv {
    _private: (), // prevent external construction
}

impl SpdkEnv {
    /// Initialize SPDK environment
    /// 
    /// # Errors
    /// Returns error if:
    /// - Already initialized (can only succeed ONCE per process)
    /// - Hugepage allocation fails
    /// - PCI access fails
    pub fn init() -> Result<SpdkEnv>;
    
    /// Builder pattern for configuration
    pub fn builder() -> SpdkEnvBuilder;
}

impl Drop for SpdkEnv {
    fn drop(&mut self) {
        // Cleans up SPDK/DPDK resources
        // WARNING: After drop, SPDK cannot be re-initialized in this process
        unsafe { spdk_env_fini(); }
    }
}

/// Builder for configuring SPDK environment
pub struct SpdkEnvBuilder {
    name: Option<String>,
    core_mask: Option<String>,
    mem_size_mb: Option<i32>,
    shm_id: Option<i32>,
    no_pci: bool,
    no_huge: bool,
    hugepage_single_segments: bool,
    main_core: Option<i32>,
}

impl SpdkEnvBuilder {
    pub fn name(self, name: &str) -> Self;
    pub fn core_mask(self, mask: &str) -> Self;
    pub fn mem_size_mb(self, mb: i32) -> Self;
    pub fn shm_id(self, id: i32) -> Self;
    pub fn no_pci(self, no_pci: bool) -> Self;
    pub fn no_huge(self, no_huge: bool) -> Self;  // vdev mode - no hugepages
    pub fn hugepage_single_segments(self, single: bool) -> Self;
    pub fn main_core(self, core: i32) -> Self;
    pub fn build(self) -> Result<SpdkEnv>;
}
```

#### Privilege Requirements

SPDK/DPDK typically requires elevated privileges for:
- **Hugepage access** - allocating/mapping hugepages
- **PCI device access** - binding to VFIO/UIO drivers  
- **Memory locking** - `mlockall()` to prevent DMA buffers from swapping

**Options for running:**

| Method | Command | Notes |
|--------|---------|-------|
| Root | `sudo ./app` | Simplest for development |
| Capabilities | `sudo setcap cap_ipc_lock,cap_sys_rawio+ep ./app` | Per-binary grant |
| Systemd | `AmbientCapabilities=CAP_IPC_LOCK CAP_SYS_RAWIO` | Production services |
| Pre-setup | Run `spdk/scripts/setup.sh` first | Prepares hugepages & drivers |

```bash
# One-time system setup (run as root)
sudo /path/to/spdk/scripts/setup.sh

# Then app may run with just capabilities (depends on config)
./my_app
```

### Thread API (modeled after std::thread)

```rust
/// SPDK thread handle - !Send, must stay on creating OS thread
/// Owns I/O channel cache and provides thread-bound operations
pub struct SpdkThread { /* NonNull<spdk_thread>, channel cache */ }

/// Handle to a spawned SPDK thread (like std::thread::JoinHandle)
pub struct JoinHandle<T> { /* ... */ }

impl<T> JoinHandle<T> {
    pub fn join(self) -> Result<T>;
    pub fn thread(&self) -> &SpdkThread;  // Get reference to the thread
}
```

### I/O Device Abstraction

```rust
/// Opaque I/O device identifier (type-safe wrapper for SPDK's void* io_device)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IoDeviceId(NonNull<c_void>);

/// Trait for types that can provide thread-local I/O channels
pub trait IoDevice {
    fn io_device_id(&self) -> IoDeviceId;
}

/// Per-thread I/O channel (obtained via SpdkThread::get_io_channel)
pub struct IoChannel { /* NonNull<spdk_io_channel> */ }
```

### Block Device API

```rust
/// Block device handle (does not own the device)
pub struct Bdev { /* NonNull<spdk_bdev> */ }

/// Open descriptor to a bdev (like file descriptor)
/// Implements IoDevice trait for channel creation
pub struct BdevDesc { /* NonNull<spdk_bdev_desc> */ }

/// DMA-capable buffer
pub struct DmaBuf { /* ptr, len */ }
```

### Blobstore API

```rust
/// Blobstore instance
/// Implements IoDevice trait for channel creation
pub struct Blobstore { /* NonNull<spdk_blob_store> */ }

/// Blob handle
pub struct Blob { /* NonNull<spdk_blob> */ }

/// Blob identifier
pub struct BlobId(spdk_blob_id);
```

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum SpdkError {
    #[error("SPDK error: {0}")]
    Errno(#[from] nix::errno::Errno),
    
    #[error("I/O operation failed")]
    IoError,
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Channel allocation failed")]
    ChannelAlloc,
    
    #[error("Operation cancelled")]
    Cancelled,
    
    // ... more variants
}

pub type Result<T> = std::result::Result<T, SpdkError>;
```

## Memory Management

### DMA Buffers

All I/O buffers must be DMA-capable (allocated via SPDK):

```rust
impl DmaBuf {
    /// Allocate DMA buffer
    pub fn alloc(size: usize, align: usize) -> Result<Self>;
    
    /// Allocate zeroed DMA buffer
    pub fn alloc_zeroed(size: usize, align: usize) -> Result<Self>;
    
    /// Get slice view
    pub fn as_slice(&self) -> &[u8];
    
    /// Get mutable slice view
    pub fn as_mut_slice(&mut self) -> &mut [u8];
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        unsafe { spdk_dma_free(self.ptr) };
    }
}
```

### Resource Cleanup

All SPDK resources implement `Drop` for RAII cleanup:

```rust
impl Drop for BdevDesc {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.ptr) };
    }
}

impl Drop for IoChannel {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.ptr) };
    }
}
```

## Thread Safety

### Send/Sync Considerations

- `SpdkThread`: `!Send + !Sync` (bound to OS thread, like `std::thread::Thread`)
- `Bdev`: `Send + Sync` (immutable reference to device metadata)
- `BdevDesc`: `Send` (can be moved between threads, but I/O needs thread's channel)
- `IoChannel`: `!Send + !Sync` (must stay on creating thread)
- `DmaBuf`: `Send` (can be moved, but not shared during I/O)

### Explicit Handle Model (like std::thread)

No thread-local statics needed. The `SpdkThread` handle is passed explicitly:

```rust
/// Opaque I/O device identifier
/// Wraps SPDK's void* io_device pointer in a type-safe way
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct IoDeviceId(pub(crate) NonNull<c_void>);

/// Trait for types that can provide an I/O channel
/// Implemented by BdevDesc, Blobstore, etc.
pub trait IoDevice {
    fn io_device_id(&self) -> IoDeviceId;
}

/// SPDK thread handle - must stay on the OS thread that created it
/// Similar to how std::thread::JoinHandle works
pub struct SpdkThread {
    ptr: NonNull<spdk_thread>,
    /// Cached I/O channels by device ID (avoids duplicate channel creation)
    channels: RefCell<HashMap<IoDeviceId, IoChannel>>,
    /// Prevent Send/Sync - must stay on creating OS thread
    _marker: PhantomData<*mut ()>,
}

impl SpdkThread {
    /// Attach an SPDK thread context to the CURRENT OS thread
    /// Does NOT create a new OS thread - you're already on one
    /// The SPDK thread is a lightweight scheduling context, not a real thread
    pub fn current(name: &str) -> Result<Self>;
    
    /// Spawn a NEW OS thread with an SPDK thread context attached
    /// Creates: 1 new OS thread + 1 SPDK thread bound to it
    /// The closure runs on the new OS thread
    pub fn spawn<F, T>(name: &str, f: F) -> JoinHandle<T>
    where
        F: FnOnce(&SpdkThread) -> T + Send + 'static,
        T: Send + 'static;
    
    /// Poll this thread's work queue
    /// Returns number of events processed (0 = idle)
    pub fn poll(&self) -> i32;
    
    /// Get or create I/O channel for a device
    /// Channels are cached - calling twice with same device returns same channel
    pub fn get_io_channel<D: IoDevice>(&self, device: &D) -> Result<&IoChannel>;
    
    /// Check if thread has pending work
    pub fn is_idle(&self) -> bool;
    
    /// Mark thread for exit and drain pending work
    pub fn exit(&self);
}

// Example IoDevice implementations:

impl IoDevice for BdevDesc {
    fn io_device_id(&self) -> IoDeviceId {
        // spdk_bdev_desc is the io_device for bdev channels
        IoDeviceId(self.ptr.cast())
    }
}

impl IoDevice for Blobstore {
    fn io_device_id(&self) -> IoDeviceId {
        IoDeviceId(self.ptr.cast())
    }
}
```

### Usage Pattern

```rust
use spdk_io::{SpdkEnv, SpdkThread};

fn main() {
    let _env = SpdkEnv::init().unwrap();
    
    // Option 1: Attach to current OS thread (no new thread)
    let thread = SpdkThread::current("main").unwrap();
    run_with_thread(&thread);
    
    // Option 2: Spawn new thread (like std::thread::spawn)
    let handle = SpdkThread::spawn("worker", |thread| {
        // thread handle passed to closure
        run_with_thread(thread)
    });
    handle.join().unwrap();
}

fn run_with_thread(thread: &SpdkThread) -> Result<()> {
    let ex = smol::LocalExecutor::new();
    futures_lite::future::block_on(ex.run(async {
        ex.spawn(poller_task(thread)).detach();
        
        // Pass thread explicitly to get channels
        let bdev = Bdev::get_by_name("Nvme0n1").await?;
        let desc = bdev.open(true).await?;
        let channel = thread.get_io_channel(&desc)?;
        
        // Use channel for I/O...
        Ok(())
    }))
}
```

## Testing

### Virtual Block Devices (vdevs)

SPDK provides virtual bdev modules for testing without real NVMe hardware:

| Module | Description | Use Case |
|--------|-------------|----------|
| **Malloc** | RAM-backed block device | Unit tests, no persistence |
| **Null** | Discards writes, returns zeros | Throughput benchmarks |
| **Error** | Injects I/O errors | Failure path testing |
| **Delay** | Adds configurable latency | Timeout testing |
| **AIO** | Linux AIO on regular files | File-backed tests |
| **Passthru** | Proxy to another bdev | Layer testing |

### Creating Test Bdevs

```rust
/// Create a RAM-backed bdev for testing
impl Bdev {
    /// Create malloc (RAM-backed) bdev
    /// Does NOT require real NVMe hardware or elevated privileges (after env init)
    pub fn create_malloc(name: &str, size_bytes: u64, block_size: u32) -> Result<Self>;
    
    /// Create null bdev (discards writes, returns zeros)
    pub fn create_null(name: &str, size_bytes: u64, block_size: u32) -> Result<Self>;
    
    /// Destroy a bdev by name
    pub fn destroy(name: &str) -> Result<()>;
}
```

### Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use spdk_io::{SpdkEnv, SpdkThread, Bdev, DmaBuf, poller_task};
    
    // Note: SPDK tests still need env init (hugepages)
    // Use a test harness that initializes once per test binary
    
    fn with_spdk<F, T>(f: F) -> T 
    where
        F: FnOnce(&SpdkThread) -> T
    {
        // In real tests, use lazy_static or ctor for one-time init
        // Use no_huge for CI without hugepage configuration
        let _env = SpdkEnv::builder()
            .name("test")
            .no_pci(true)
            .no_huge(true)
            .mem_size_mb(64)
            .build()
            .unwrap();
        
        let thread = SpdkThread::current("test").unwrap();
        f(&thread)
    }
    
    #[test]
    fn test_read_write_malloc() {
        with_spdk(|thread| {
            let ex = smol::LocalExecutor::new();
            futures_lite::future::block_on(ex.run(async {
                ex.spawn(poller_task(thread)).detach();
                
                // Create 1MB RAM-backed bdev
                let bdev = Bdev::create_malloc("test0", 1024 * 1024, 4096).unwrap();
                let desc = bdev.open(true).await.unwrap();
                let channel = thread.get_io_channel(&desc).unwrap();
                
                // Write pattern
                let mut buf = DmaBuf::alloc(4096, 4096).unwrap();
                buf.as_mut_slice().fill(0xAB);
                desc.write(channel, &buf, 0, 4096).await.unwrap();
                
                // Read back and verify
                let mut read_buf = DmaBuf::alloc(4096, 4096).unwrap();
                desc.read(channel, &mut read_buf, 0, 4096).await.unwrap();
                assert_eq!(read_buf.as_slice(), buf.as_slice());
                
                // Cleanup
                drop(desc);
                Bdev::destroy("test0").unwrap();
            }));
        });
    }
}
```

### Integration Testing with Real Devices

For tests requiring actual NVMe:
```rust
#[test]
#[ignore]  // Run with: cargo test -- --ignored
fn test_with_real_nvme() {
    // Requires: sudo, NVMe device bound to SPDK
    let bdev = Bdev::get_by_name("Nvme0n1").await.unwrap();
    // ...
}
```

## Future Considerations

### Phase 1: Core Functionality
- [ ] spdk-io-sys bindings generation
- [ ] Environment initialization
- [ ] SPDK thread creation/management
- [ ] Bdev open/close/read/write
- [ ] Runtime-agnostic poller task
- [ ] DMA buffer management
- [ ] Callback-to-future utilities

### Phase 2: Extended APIs
- [ ] Blobstore support
- [ ] NVMe driver direct access
- [ ] Multiple bdev modules (malloc, null, aio)
- [ ] Better error context

### Phase 3: Advanced Features
- [ ] Multi-threaded coordination utilities
- [ ] Reactor affinity helpers
- [ ] Custom poller integration
- [ ] Tracing/metrics
- [ ] Optional Tokio/smol convenience wrappers

## Dependencies

```toml
[dependencies]
spdk-io-sys = { path = "../spdk-io-sys" }

# Async utilities (runtime-agnostic)
futures-util = "0.3"
futures-channel = "0.3"
futures-core = "0.3"

# Error handling
thiserror = "1"
nix = { version = "0.27", features = ["fs"] }

# Optional: pin utilities
pin-project-lite = "0.2"

[dev-dependencies]
# For testing with Tokio
tokio = { version = "1", features = ["rt", "macros"] }
# For testing with smol
smol = "2"
futures-lite = "2"

[build-dependencies]
bindgen = "0.69"
pkg-config = "0.3"
```

## References

- [SPDK Documentation](https://spdk.io/doc/)
- [futures-rs](https://docs.rs/futures/latest/futures/) - Core async utilities
- [futures-util](https://docs.rs/futures-util/latest/futures_util/) - Future combinators
- [Background.md](Background.md) - SPDK concepts and APIs
- [Reference.md](Reference.md) - Existing Rust SPDK projects
