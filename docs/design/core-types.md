# Core Types

## Environment & Initialization

### SpdkEnv

```rust
/// SPDK environment guard - initialized ONCE per process
/// 
/// Like DPDK's EAL, the SPDK environment can only be initialized once.
/// This struct acts as a guard: when dropped, SPDK is cleaned up but
/// CANNOT be re-initialized (DPDK limitation).
pub struct SpdkEnv {
    _private: (),
}

impl SpdkEnv {
    pub fn init() -> Result<SpdkEnv>;
    pub fn builder() -> SpdkEnvBuilder;
}

impl Drop for SpdkEnv {
    fn drop(&mut self) {
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
```

> **Note:** `SpdkEnv` only initializes the DPDK environment. For bdev subsystem and
> JSON configuration support, use `SpdkApp` instead.

### Privilege Requirements

SPDK/DPDK typically requires elevated privileges for:
- **Hugepage access** - allocating/mapping hugepages
- **PCI device access** - binding to VFIO/UIO drivers  
- **Memory locking** - `mlockall()` to prevent DMA buffers from swapping

| Method | Command | Notes |
|--------|---------|-------|
| Root | `sudo ./app` | Simplest for development |
| Capabilities | `sudo setcap cap_ipc_lock,cap_sys_rawio+ep ./app` | Per-binary grant |
| Systemd | `AmbientCapabilities=CAP_IPC_LOCK CAP_SYS_RAWIO` | Production services |
| Pre-setup | Run `spdk/scripts/setup.sh` first | Prepares hugepages & drivers |

## Application Framework (SpdkApp)

Most SPDK applications should use the **SPDK Application Framework** which handles:

1. Environment initialization (DPDK/hugepages)
2. All subsystem initialization (bdev, nvmf, etc.)
3. JSON configuration loading (bdevs, etc.)
4. Reactor/poller infrastructure
5. Signal handling and graceful shutdown

### SpdkApp vs SpdkEnv

| Feature | `SpdkEnv` (low-level) | `SpdkApp` (framework) |
|---------|----------------------|----------------------|
| Init via | `spdk_env_init()` | `spdk_app_start()` |
| Subsystems | Manual init required | Auto-initialized |
| JSON config | Not supported | Native support |
| Bdev creation | Need internal headers | Via JSON config |
| Main loop | User-managed | Framework-managed |
| Use case | Embedding, custom apps | Typical SPDK apps |

### SpdkAppBuilder

```rust
impl SpdkAppBuilder {
    pub fn name(self, name: &str) -> Self;
    pub fn config_file(self, path: &str) -> Self;
    pub fn json_data(self, json: &str) -> Self;
    pub fn reactor_mask(self, mask: &str) -> Self;
    pub fn main_core(self, core: i32) -> Self;
    pub fn mem_size_mb(self, mb: i32) -> Self;
    pub fn no_pci(self, no_pci: bool) -> Self;
    pub fn no_huge(self, no_huge: bool) -> Self;
    pub fn rpc_addr(self, addr: &str) -> Self;
    
    /// Run with synchronous callback
    pub fn run<F>(self, f: F) -> Result<()>
    where F: FnOnce() + 'static;
    
    /// Run with async main function
    pub fn run_async<F, Fut>(self, f: F) -> Result<()>
    where
        F: FnOnce() -> Fut + 'static,
        Fut: Future<Output = ()> + 'static;
}
```

### Example

```rust
SpdkApp::builder()
    .name("my_app")
    .json_data(r#"{
        "subsystems": [{
            "subsystem": "bdev",
            "config": [{
                "method": "bdev_null_create",
                "params": {"name": "Null0", "num_blocks": 1024, "block_size": 512}
            }]
        }]
    }"#)
    .run(|| {
        let bdev = Bdev::get_by_name("Null0").unwrap();
        // ... use bdev
        SpdkApp::stop();
    })?;
```

## Thread API

```rust
/// SPDK thread context - !Send + !Sync, must stay on creating OS thread.
/// This is a lightweight scheduling context, NOT an OS thread.
pub struct SpdkThread {
    ptr: NonNull<spdk_thread>,
    _marker: PhantomData<*mut ()>,
}

impl SpdkThread {
    pub fn current(name: &str) -> Result<Self>;
    pub fn new(name: &str) -> Result<Self>;
    pub fn current_with_mempool_size(name: &str, size: usize) -> Result<Self>;
    pub fn get_current() -> Option<CurrentThread>;
    pub fn app_thread() -> Option<CurrentThread>;
    
    pub fn poll(&self) -> i32;
    pub fn poll_max(&self, max_msgs: u32) -> i32;
    
    pub fn has_active_pollers(&self) -> bool;
    pub fn has_pollers(&self) -> bool;
    pub fn is_idle(&self) -> bool;
    pub fn is_running(&self) -> bool;
    pub fn name(&self) -> &str;
    pub fn id(&self) -> u64;
    pub fn count() -> u32;
    
    pub fn spawn<F, T>(name: &str, f: F) -> JoinHandle<T>
    where
        F: FnOnce(&SpdkThread) -> T + Send + 'static,
        T: Send + 'static;
    
    pub fn handle(&self) -> ThreadHandle;
}
```

## Cross-Thread Messaging

```rust
/// Thread-safe handle for sending messages to an SPDK thread.
/// Unlike SpdkThread (which is !Send), this can be sent across threads.
#[derive(Clone)]
pub struct ThreadHandle {
    ptr: *const spdk_thread,
}

unsafe impl Send for ThreadHandle {}
unsafe impl Sync for ThreadHandle {}

impl ThreadHandle {
    /// Send a closure to execute on the target thread.
    pub fn send<F>(&self, f: F)
    where F: FnOnce() + Send + 'static;

    /// Send a closure and await the result.
    pub fn call<F, T>(&self, f: F) -> CompletionReceiver<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static;

    pub fn id(&self) -> u64;
}
```

## I/O Channel

```rust
/// Per-thread I/O channel.
/// Must be created and used on the same OS thread.
/// 
/// # Thread Safety
/// `!Send + !Sync` - must stay on creating thread.
pub struct IoChannel {
    ptr: NonNull<spdk_io_channel>,
    _marker: PhantomData<*mut ()>,
}

impl IoChannel {
    pub fn thread(&self) -> CurrentThread;
    pub fn as_ptr(&self) -> *mut spdk_io_channel;
}

impl Drop for IoChannel {
    fn drop(&mut self) {
        unsafe { spdk_put_io_channel(self.ptr.as_ptr()) };
    }
}
```

## DMA Buffer

```rust
/// DMA-capable buffer for I/O operations.
pub struct DmaBuf {
    ptr: *mut u8,
    size: usize,
}

impl DmaBuf {
    pub fn alloc(size: usize, align: usize) -> Result<Self>;
    pub fn alloc_zeroed(size: usize, align: usize) -> Result<Self>;
    pub fn as_slice(&self) -> &[u8];
    pub fn as_mut_slice(&mut self) -> &mut [u8];
    pub fn len(&self) -> usize;
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        unsafe { spdk_dma_free(self.ptr as *mut c_void) };
    }
}
```

## Completion Utilities

```rust
/// Create a completion sender/receiver pair.
pub fn completion<T>() -> (CompletionSender<T>, CompletionReceiver<T>);

/// Block on a future, polling the SPDK thread while waiting.
pub fn block_on<F, T>(future: F) -> T
where F: Future<Output = T>;
```
