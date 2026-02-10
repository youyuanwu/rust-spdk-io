# Testing

## Virtual Block Devices (vdevs)

SPDK provides virtual bdev modules for testing without real NVMe hardware:

| Module | Description | Use Case |
|--------|-------------|----------|
| **Malloc** | RAM-backed block device | Unit tests, no persistence |
| **Null** | Discards writes, returns zeros | Throughput benchmarks |
| **Error** | Injects I/O errors | Failure path testing |
| **Delay** | Adds configurable latency | Timeout testing |
| **AIO** | Linux AIO on regular files | File-backed tests |
| **Passthru** | Proxy to another bdev | Layer testing |

## Creating Test Bdevs

Test bdevs are created via JSON config:

```rust
SpdkApp::builder()
    .name("test")
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
        // ...
        SpdkApp::stop();
    })?;
```

JSON config methods for bdevs:
- `bdev_null_create` - discards writes, returns zeros (simplest)
- `bdev_malloc_create` - RAM-backed block device
- `bdev_error_create` - injects I/O errors
- `bdev_delay_create` - adds configurable latency

## Unit Test Example

```rust
#[cfg(test)]
mod tests {
    use spdk_io::{SpdkApp, Bdev};
    
    #[test]
    fn test_null_bdev() {
        SpdkApp::builder()
            .name("test")
            .no_pci(true)
            .no_huge(true)
            .mem_size_mb(256)
            .json_data(r#"{
                "subsystems": [{
                    "subsystem": "bdev",
                    "config": [{
                        "method": "bdev_null_create",
                        "params": {"name": "test0", "num_blocks": 1024, "block_size": 512}
                    }]
                }]
            }"#)
            .run(|| {
                let bdev = Bdev::get_by_name("test0").unwrap();
                assert_eq!(bdev.name(), "test0");
                assert_eq!(bdev.block_size(), 512);
                assert_eq!(bdev.num_blocks(), 1024);
                
                let desc = bdev.open(true).unwrap();
                let channel = desc.get_io_channel().unwrap();
                
                drop(channel);
                drop(desc);
                
                SpdkApp::stop();
            })
            .expect("SPDK test failed");
    }
}
```

## Async Test Example

```rust
#[test]
fn test_async_bdev_io() {
    SpdkApp::builder()
        .name("test_async")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(256)
        .json_data(/* config */)
        .run_async(|| async {
            let bdev = Bdev::get_by_name("test0").unwrap();
            let desc = bdev.open(true).unwrap();
            let channel = desc.get_io_channel().unwrap();
            
            let mut buf = DmaBuf::alloc(512, 512).unwrap();
            desc.read(&channel, &mut buf, 0, 512).await.unwrap();
            
            // Verify zeros (null bdev)
            assert!(buf.as_slice().iter().all(|&b| b == 0));
            
            SpdkApp::stop();
        })
        .expect("Async test failed");
}
```

## NVMf Subprocess Testing

See [nvmf.md](nvmf.md) for the recommended subprocess approach for NVMf testing.

```rust
#[test]
fn test_nvmf_subprocess() {
    const TEST_PORT: u16 = 4421;
    
    nvmf_subprocess::NvmfTarget::cleanup_stale(TEST_PORT);
    let (target, nqn) = nvmf_subprocess::NvmfTarget::start(TEST_PORT).unwrap();
    
    SpdkApp::builder()
        .name("test")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(1024)
        .run(|| {
            let trid = TransportId::tcp("127.0.0.1", &TEST_PORT.to_string(), &nqn).unwrap();
            let ctrlr = NvmeController::connect(&trid, None).unwrap();
            // ... I/O operations ...
            SpdkApp::stop();
        })
        .unwrap();
}
```

## Integration Testing with Real Devices

For tests requiring actual NVMe:

```rust
#[test]
#[ignore]  // Run with: cargo test -- --ignored
fn test_with_real_nvme() {
    // Requires: sudo, NVMe device bound to SPDK
    let pci_addr = std::env::var("NVME_PCI_ADDR")
        .unwrap_or_else(|_| "0000:00:04.0".to_string());
    
    SpdkApp::builder()
        .name("nvme_test")
        .run_async(|| async {
            let trid = TransportId::pcie(&pci_addr).unwrap();
            let ctrlr = NvmeController::connect(&trid, None).unwrap();
            // ...
            SpdkApp::stop();
        })
        .unwrap();
}
```

## vdev Mode (No Hugepages)

For testing without hugepages:

```rust
SpdkApp::builder()
    .name("test")
    .no_pci(true)     // Don't scan PCI
    .no_huge(true)    // Don't require hugepages
    .mem_size_mb(256) // Use regular memory
    .run(|| { /* ... */ })
```
