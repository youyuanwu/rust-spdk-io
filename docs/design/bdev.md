# Block Device API

## Overview

The bdev (block device) layer provides a uniform interface over various storage backends.

## Bdev

```rust
/// Block device handle (does not own the device).
/// 
/// Obtained via `Bdev::get_by_name()` after bdevs are created via JSON config.
/// The bdev itself is managed by SPDK's bdev layer.
/// 
/// # Thread Safety
/// `!Send + !Sync` - conservative default.
pub struct Bdev {
    ptr: NonNull<spdk_bdev>,
    _marker: PhantomData<*mut ()>,
}

impl Bdev {
    /// Look up a bdev by name.
    pub fn get_by_name(name: &str) -> Option<Self>;
    
    /// Open this bdev for I/O.
    pub fn open(&self, write: bool) -> Result<BdevDesc>;
    
    /// Get bdev name.
    pub fn name(&self) -> &str;
    
    /// Get block size in bytes.
    pub fn block_size(&self) -> u32;
    
    /// Get number of blocks.
    pub fn num_blocks(&self) -> u64;
    
    /// Get total size in bytes.
    pub fn size_bytes(&self) -> u64;
}
```

## BdevDesc

```rust
/// Open descriptor to a bdev (like a file descriptor).
/// 
/// Use `get_io_channel()` to obtain a thread-local channel for I/O.
/// Must be closed on the same thread it was opened on.
/// 
/// # Thread Safety
/// `!Send + !Sync` - must stay on opening thread for close.
pub struct BdevDesc {
    ptr: NonNull<spdk_bdev_desc>,
    _marker: PhantomData<*mut ()>,
}

impl BdevDesc {
    /// Get an I/O channel for this descriptor on the current thread.
    pub fn get_io_channel(&self) -> Result<IoChannel>;
    
    /// Get the underlying bdev.
    pub fn bdev(&self) -> Bdev;
    
    /// Async read operation.
    pub async fn read(
        &self,
        channel: &IoChannel,
        buf: &mut DmaBuf,
        offset: u64,
        len: u64,
    ) -> Result<()>;
    
    /// Async write operation.
    pub async fn write(
        &self,
        channel: &IoChannel,
        buf: &DmaBuf,
        offset: u64,
        len: u64,
    ) -> Result<()>;
}

impl Drop for BdevDesc {
    fn drop(&mut self) {
        unsafe { spdk_bdev_close(self.ptr.as_ptr()) };
    }
}
```

## Creating Bdevs

Bdevs are created at SPDK init time via JSON config:

```rust
SpdkApp::builder()
    .name("app")
    .json_data(r#"{
        "subsystems": [{
            "subsystem": "bdev",
            "config": [{
                "method": "bdev_null_create",
                "params": {"name": "Null0", "num_blocks": 262144, "block_size": 512}
            }]
        }]
    }"#)
    .run(|| {
        let bdev = Bdev::get_by_name("Null0").unwrap();
        // ... use bdev
        SpdkApp::stop();
    })?;
```

## Available Bdev Types

| Method | Description | Use Case |
|--------|-------------|----------|
| `bdev_null_create` | Discards writes, returns zeros | Throughput benchmarks |
| `bdev_malloc_create` | RAM-backed block device | Unit tests |
| `bdev_error_create` | Injects I/O errors | Failure path testing |
| `bdev_delay_create` | Adds configurable latency | Timeout testing |
| `bdev_aio_create` | Linux AIO on regular files | File-backed tests |

## Example: Async I/O

```rust
SpdkApp::builder()
    .name("test")
    .json_data(/* config with bdev */)
    .run_async(|| async {
        let bdev = Bdev::get_by_name("Null0").unwrap();
        let desc = bdev.open(true).unwrap();
        let channel = desc.get_io_channel().unwrap();
        
        let mut buf = DmaBuf::alloc(4096, 4096).unwrap();
        
        // Write data
        buf.as_mut_slice().fill(0xAB);
        desc.write(&channel, &buf, 0, 4096).await.unwrap();
        
        // Read back
        buf.as_mut_slice().fill(0x00);
        desc.read(&channel, &mut buf, 0, 4096).await.unwrap();
        
        SpdkApp::stop();
    })
    .unwrap();
```
