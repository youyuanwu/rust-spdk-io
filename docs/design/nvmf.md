# NVMf Target API

SPDK exposes the NVMe-oF target as a library, allowing target + initiator to run
**in the same process**. This enables NVMe driver testing without real hardware.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Same Process (spdk-io test)                                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────┐      ┌─────────────────────────┐  │
│  │  NVMf Target        │      │  NVMe Initiator         │  │
│  │                     │      │                         │  │
│  │  Null Bdev          │◄────►│  NvmeController         │  │
│  │    ▼                │ TCP  │    ▼                    │  │
│  │  Subsystem          │loopbk│  NvmeNamespace          │  │
│  │    ▼                │      │    ▼                    │  │
│  │  TCP Listener       │      │  NvmeQpair              │  │
│  │  127.0.0.1:4420     │      │                         │  │
│  └─────────────────────┘      └─────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## ⚠️ Threading Warning

**In-process NVMf targets can have threading issues.** Running the NVMf target and NVMe
initiator on the same SPDK thread can cause deadlocks. SPDK expects target and initiator
to run on separate reactor cores.

**Recommended:** Use the subprocess approach for testing (see below).

## Subprocess Testing (Recommended)

For testing, spawn `nvmf_tgt` as a separate process. This avoids threading issues
and provides better isolation. See `tests/nvmf_test.rs` for the full implementation.

```rust
/// Helper to manage nvmf_tgt subprocess
mod nvmf_subprocess {
    pub struct NvmfTarget { /* ... */ }
    
    impl NvmfTarget {
        /// Clean up stale processes from previous runs via PID file
        pub fn cleanup_stale(port: u16);
        
        /// Start nvmf_tgt subprocess, configure via RPC
        pub fn start(port: u16) -> Result<(Self, String), String>;
    }
}

#[test]
fn test_nvmf_subprocess() {
    const TEST_PORT: u16 = 4421;
    
    // Clean up any stale processes
    nvmf_subprocess::NvmfTarget::cleanup_stale(TEST_PORT);
    
    // Start nvmf_tgt as subprocess
    let (target, nqn) = nvmf_subprocess::NvmfTarget::start(TEST_PORT).unwrap();
    
    SpdkApp::builder()
        .name("test")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(1024)
        .run(|| {
            // Connect to external nvmf_tgt
            let trid = TransportId::tcp("127.0.0.1", &TEST_PORT.to_string(), &nqn).unwrap();
            let ctrlr = NvmeController::connect(&trid, None).unwrap();
            
            // Perform I/O...
            
            SpdkApp::stop();
        })
        .unwrap();
    
    // target dropped here, kills subprocess
}
```

## In-Process API (Use with Caution)

### NvmfTarget

```rust
pub struct NvmfTarget {
    ptr: NonNull<spdk_nvmf_tgt>,
    _marker: PhantomData<*mut ()>,
}

impl NvmfTarget {
    pub fn create(name: &str) -> Result<Self>;
    pub async fn add_transport(&self, transport: NvmfTransport) -> Result<()>;
    pub fn create_subsystem(&self, nqn: &str, opts: NvmfSubsystemOpts) -> Result<NvmfSubsystem>;
    pub fn get_subsystem(&self, nqn: &str) -> Option<NvmfSubsystem>;
}
```

### NvmfTransport

```rust
pub struct NvmfTransport {
    ptr: NonNull<spdk_nvmf_transport>,
}

impl NvmfTransport {
    pub fn tcp(opts: Option<&NvmfTransportOpts>) -> Result<Self>;
    pub fn rdma(opts: Option<&NvmfTransportOpts>) -> Result<Self>;
}

#[derive(Debug, Default, Clone)]
pub struct NvmfTransportOpts {
    pub max_io_size: Option<u32>,
    pub io_unit_size: Option<u32>,
    pub max_qpairs_per_ctrlr: Option<u16>,
    pub in_capsule_data_size: Option<u32>,
}
```

### NvmfSubsystem

```rust
pub struct NvmfSubsystem {
    ptr: NonNull<spdk_nvmf_subsystem>,
}

impl NvmfSubsystem {
    pub fn add_namespace(&self, bdev_name: &str) -> Result<u32>;
    pub async fn add_listener(&self, trid: &TransportId) -> Result<()>;
    pub fn allow_any_host(&self, allow: bool);
    pub async fn start(&self) -> Result<()>;
    pub async fn stop(&self) -> Result<()>;
    pub fn nqn(&self) -> &str;
}

#[derive(Debug, Default, Clone)]
pub struct NvmfSubsystemOpts {
    pub serial_number: Option<String>,
    pub model_number: Option<String>,
    pub allow_any_host: bool,
}
```

## Module Structure

```
spdk-io/src/nvmf/
├── mod.rs        # Module exports
├── target.rs     # NvmfTarget
├── transport.rs  # NvmfTransport
├── subsystem.rs  # NvmfSubsystem
└── opts.rs       # NvmfTransportOpts, NvmfSubsystemOpts
```
