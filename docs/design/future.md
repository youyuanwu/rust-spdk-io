# Future & References

## Completed Features

- [x] spdk-io-sys bindings generation
- [x] Environment initialization (`SpdkEnv`, `SpdkApp`)
- [x] SPDK thread creation/management (`SpdkThread`, `ThreadHandle`)
- [x] Bdev open/close/read/write
- [x] Runtime-agnostic poller task (`spdk_poller`, `block_on`)
- [x] DMA buffer management (`DmaBuf`)
- [x] Callback-to-future utilities (`completion`, `io_completion`)
- [x] NVMe driver direct access (`nvme` module)
- [x] NVMf target API (`nvmf` module)
- [x] Cross-thread messaging (`ThreadHandle::send`, `ThreadHandle::call`)
- [x] Thread spawning (`SpdkThread::spawn`, `JoinHandle`)

## Planned Features

- [ ] **Blobstore support** - `Blobstore`, `Blob`, `BlobId` types
- [ ] **Poller API** - `Poller::register()`, `pause()`, `resume()` for periodic callbacks
- [ ] **CpuMask/CpuSet** - CPU affinity mask for thread pinning
- [ ] **Better error context** - Error spans for debugging
- [ ] **Tracing/metrics** - Observability integration
- [ ] **Runtime wrappers** - Optional Tokio/smol convenience

## Blobstore API (Planned)

```rust
/// Blobstore instance.
pub struct Blobstore {
    ptr: NonNull<spdk_blob_store>,
}

impl Blobstore {
    pub async fn load(bdev: &Bdev) -> Result<Self>;
    pub async fn init(bdev: &Bdev) -> Result<Self>;
    pub fn alloc_io_channel(&self) -> Result<IoChannel>;
    pub async fn create_blob(&self) -> Result<BlobId>;
    pub async fn open_blob(&self, id: BlobId) -> Result<Blob>;
    pub async fn delete_blob(&self, id: BlobId) -> Result<()>;
}

/// Blob handle.
pub struct Blob {
    ptr: NonNull<spdk_blob>,
}

impl Blob {
    pub async fn read(&self, channel: &IoChannel, buf: &mut DmaBuf, offset: u64) -> Result<()>;
    pub async fn write(&self, channel: &IoChannel, buf: &DmaBuf, offset: u64) -> Result<()>;
    pub async fn resize(&self, num_clusters: u64) -> Result<()>;
    pub async fn sync(&self) -> Result<()>;
    pub fn get_xattr(&self, name: &str) -> Result<Vec<u8>>;
    pub fn set_xattr(&self, name: &str, value: &[u8]) -> Result<()>;
}

/// Blob identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobId(spdk_blob_id);
```

## Poller API (Planned)

SPDK pollers are periodic callbacks that run on the reactor's event loop. Instead of
blocking or sleeping, pollers are called repeatedly by the reactor.

```rust
/// SPDK poller handle.
/// 
/// Pollers are registered on the current thread's reactor and called
/// periodically. The callback returns `true` if work was done.
pub struct Poller {
    ptr: *mut spdk_poller,
}

impl Poller {
    /// Register a poller on the current reactor.
    ///
    /// The closure is called repeatedly. Return `true` if work was done,
    /// `false` otherwise. SPDK uses return value for busy-polling optimization.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let poller = Poller::register(|| {
    ///     let completions = qpair.process_completions(0);
    ///     completions > 0
    /// })?;
    /// ```
    pub fn register<F>(f: F) -> Result<Self>
    where
        F: Fn() -> bool + 'static;
    
    /// Register a timed poller (called at fixed interval).
    pub fn register_timed<F>(f: F, period: Duration) -> Result<Self>
    where
        F: Fn() -> bool + 'static;
    
    /// Pause the poller (stops being called until resumed).
    pub fn pause(&mut self);
    
    /// Resume a paused poller.
    pub fn resume(&mut self);
    
    /// Unregister the poller (also happens on Drop).
    pub fn unregister(self);
}

impl Drop for Poller {
    fn drop(&mut self) {
        // spdk_poller_unregister(&mut self.ptr)
    }
}
```

### Use Cases

1. **Completion polling** - Check for I/O completions (`qpair.process_completions()`)
2. **Async executor** - Drive futures by polling them in a poller
3. **Periodic tasks** - Health checks, stats, timeouts
4. **TCP transport** - NVMe/TCP requires polling for network events

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
libc = "0.2"

[build-dependencies]
bindgen = "0.69"
pkg-config = "0.3"
```

## References

- [SPDK Documentation](https://spdk.io/doc/)
- [futures-rs](https://docs.rs/futures/latest/futures/) - Core async utilities
- [futures-util](https://docs.rs/futures-util/latest/futures_util/) - Future combinators
- [Background.md](../Background.md) - SPDK concepts and APIs
- [Reference.md](../Reference.md) - Existing Rust SPDK projects
