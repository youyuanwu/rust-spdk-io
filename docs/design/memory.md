# Memory & Thread Safety

## DMA Buffers

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

## Resource Cleanup

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

impl Drop for NvmeController {
    fn drop(&mut self) {
        unsafe { spdk_nvme_detach(self.ptr.as_ptr()) };
    }
}

impl Drop for NvmeQpair {
    fn drop(&mut self) {
        unsafe { spdk_nvme_ctrlr_free_io_qpair(self.ptr.as_ptr()) };
    }
}
```

## Send/Sync Considerations

| Type | Send | Sync | Notes |
|------|------|------|-------|
| `SpdkThread` | ❌ | ❌ | Bound to OS thread |
| `CurrentThread` | ❌ | ❌ | Borrowed reference |
| `ThreadHandle` | ✅ | ✅ | Thread-safe messaging |
| `Bdev` | ❌ | ❌ | Conservative default |
| `BdevDesc` | ❌ | ❌ | Must close on opening thread |
| `IoChannel` | ❌ | ❌ | Must stay on creating thread |
| `DmaBuf` | ✅ | ❌ | Can be moved, not shared during I/O |
| `NvmeController` | ❌ | ❌ | Operations on connecting thread |
| `NvmeQpair` | ❌ | ❌ | Must stay on allocating thread |
| `NvmeNamespace` | ❌ | ❌ | Borrowed from controller |

## Explicit Handle Model

No thread-local statics needed. Channels are obtained from the device, not the thread:

```rust
// Channel acquisition is per-device, not per-thread:

impl BdevDesc {
    /// Get thread-local I/O channel for this device
    pub fn get_io_channel(&self) -> Result<IoChannel>;
}

impl Blobstore {
    /// Allocate thread-local I/O channel for this blobstore
    pub fn alloc_io_channel(&self) -> Result<IoChannel>;
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    
    #[error("NVMe error: SCT={sct}, SC={sc}")]
    NvmeError { sct: u8, sc: u8 },
    
    #[error("NVMe controller not found")]
    ControllerNotFound,
    
    #[error("NVMe namespace not found: {0}")]
    NamespaceNotFound(u32),
    
    #[error("NVMe qpair allocation failed")]
    QpairAlloc,
    
    // ... more variants
}

pub type Result<T> = std::result::Result<T, Error>;
```

## Usage Pattern

```rust
use spdk_io::{SpdkEnv, SpdkThread};

fn main() {
    let _env = SpdkEnv::init().unwrap();
    
    // Option 1: Attach to current OS thread
    let thread = SpdkThread::current("main").unwrap();
    run_with_thread(&thread);
    
    // Option 2: Spawn new thread
    let handle = SpdkThread::spawn("worker", |thread| {
        run_with_thread(thread)
    });
    handle.join().unwrap();
}

fn run_with_thread(thread: &SpdkThread) -> Result<()> {
    let ex = smol::LocalExecutor::new();
    futures_lite::future::block_on(ex.run(async {
        ex.spawn(poller_task(thread)).detach();
        
        let bdev = Bdev::get_by_name("Nvme0n1")
            .ok_or(Error::DeviceNotFound("Nvme0n1".into()))?;
        let desc = bdev.open(true)?;
        
        // Get I/O channel FROM THE DESCRIPTOR
        let channel = desc.get_io_channel()?;
        
        let mut buf = DmaBuf::alloc(4096, 4096)?;
        desc.read(&channel, &mut buf, 0, 4096).await?;
        
        Ok(())
    }))
}
```
