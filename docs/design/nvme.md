# NVMe API

Direct NVMe access bypassing the bdev layer. Use this for:
- Custom admin commands
- Namespace management
- Maximum performance (bypasses bdev abstraction)
- E2E tests with real NVMe devices

## TransportId

```rust
/// NVMe transport identifier.
/// Identifies how to connect to an NVMe controller (PCIe, TCP, RDMA, etc.)
#[derive(Debug, Clone)]
pub struct TransportId {
    inner: spdk_nvme_transport_id,
}

impl TransportId {
    /// Create a PCIe transport ID from BDF address.
    pub fn pcie(addr: &str) -> Result<Self>;
    
    /// Create a TCP transport ID.
    pub fn tcp(addr: &str, port: &str, subnqn: &str) -> Result<Self>;
    
    /// Create a RDMA transport ID.
    pub fn rdma(addr: &str, port: &str, subnqn: &str) -> Result<Self>;
    
    /// Parse from string (SPDK format).
    pub fn parse(s: &str) -> Result<Self>;
}
```

## NvmeController

```rust
/// NVMe controller handle.
/// 
/// # Thread Safety
/// `!Send + !Sync` - operations must remain on the thread that connected.
pub struct NvmeController {
    ptr: NonNull<spdk_nvme_ctrlr>,
    _marker: PhantomData<*mut ()>,
}

impl NvmeController {
    /// Connect to an NVMe controller.
    pub fn connect(trid: &TransportId, opts: Option<&NvmeCtrlrOpts>) -> Result<Self>;
    
    /// Connect asynchronously.
    pub async fn connect_async(trid: &TransportId, opts: Option<&NvmeCtrlrOpts>) -> Result<Self>;
    
    /// Get the number of namespaces.
    pub fn num_namespaces(&self) -> u32;
    
    /// Get a namespace by ID (1-indexed).
    pub fn namespace(&self, ns_id: u32) -> Option<NvmeNamespace>;
    
    /// Allocate an I/O queue pair.
    pub fn alloc_io_qpair(&self, opts: Option<&NvmeQpairOpts>) -> Result<NvmeQpair>;
    
    /// Process admin command completions.
    pub fn process_admin_completions(&self) -> i32;
    
    /// Get controller identify data.
    pub fn data(&self) -> &spdk_nvme_ctrlr_data;
}
```

## NvmeNamespace

```rust
/// NVMe namespace handle.
/// 
/// The namespace is borrowed from the controller and becomes invalid
/// when the controller is dropped.
pub struct NvmeNamespace<'a> {
    ptr: NonNull<spdk_nvme_ns>,
    ctrlr: &'a NvmeController,
}

impl<'a> NvmeNamespace<'a> {
    pub fn id(&self) -> u32;
    pub fn sector_size(&self) -> u32;
    pub fn num_sectors(&self) -> u64;
    pub fn size(&self) -> u64;
    pub fn max_io_xfer_size(&self) -> u32;
    pub fn is_active(&self) -> bool;
    
    /// Submit a read command.
    pub async fn read(
        &self, 
        qpair: &NvmeQpair, 
        buf: &mut DmaBuf, 
        lba: u64, 
        num_blocks: u32
    ) -> Result<()>;
    
    /// Submit a write command.
    pub async fn write(
        &self, 
        qpair: &NvmeQpair, 
        buf: &DmaBuf, 
        lba: u64, 
        num_blocks: u32
    ) -> Result<()>;
    
    /// Submit a flush command.
    pub async fn flush(&self, qpair: &NvmeQpair) -> Result<()>;
}
```

## NvmeQpair

```rust
/// NVMe I/O queue pair.
/// Each thread should have its own qpair for lock-free operation.
/// 
/// # Thread Safety
/// `!Send + !Sync` - qpair must stay on the allocating thread.
pub struct NvmeQpair {
    ptr: NonNull<spdk_nvme_qpair>,
    ctrlr_ptr: *mut spdk_nvme_ctrlr,
    _marker: PhantomData<*mut ()>,
}

impl NvmeQpair {
    /// Process I/O completions.
    pub fn process_completions(&self, max_completions: u32) -> i32;
}
```

## Options

```rust
#[derive(Debug, Default, Clone)]
pub struct NvmeCtrlrOpts {
    pub num_io_queues: Option<u32>,
    pub io_queue_size: Option<u32>,
    pub admin_queue_size: Option<u16>,
    pub keep_alive_timeout_ms: Option<u32>,
}

#[derive(Debug, Default, Clone)]
pub struct NvmeQpairOpts {
    pub io_queue_size: Option<u32>,
    pub io_queue_requests: Option<u32>,
}
```

## Example

```rust
use spdk_io::{SpdkApp, DmaBuf, nvme::{NvmeController, TransportId}};

SpdkApp::builder()
    .name("nvme_test")
    .run_async(|| async {
        // Connect to NVMe controller
        let trid = TransportId::pcie("0000:00:04.0").unwrap();
        let ctrlr = NvmeController::connect(&trid, None).unwrap();
        
        // Get namespace 1
        let ns = ctrlr.namespace(1).expect("NS1 not found");
        let sector_size = ns.sector_size() as usize;
        
        // Allocate qpair and buffer
        let qpair = ctrlr.alloc_io_qpair(None).unwrap();
        let mut buf = DmaBuf::alloc(sector_size, sector_size).unwrap();
        
        // Write test pattern
        buf.as_mut_slice().fill(0xAB);
        ns.write(&qpair, &buf, 0, 1).await.unwrap();
        
        // Read back and verify
        buf.as_mut_slice().fill(0x00);
        ns.read(&qpair, &mut buf, 0, 1).await.unwrap();
        assert!(buf.as_slice().iter().all(|&b| b == 0xAB));
        
        SpdkApp::stop();
    })
    .unwrap();
```

## Module Structure

```
spdk-io/src/nvme/
├── mod.rs        # Module exports
├── controller.rs # NvmeController
├── namespace.rs  # NvmeNamespace
├── qpair.rs      # NvmeQpair
├── transport.rs  # TransportId
└── opts.rs       # NvmeCtrlrOpts, NvmeQpairOpts
```
