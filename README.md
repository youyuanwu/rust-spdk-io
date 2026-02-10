# spdk-io

High-level async Rust bindings for [SPDK](https://spdk.io/) (Storage Performance Development Kit).

## Overview

**spdk-io** enables Rust applications to leverage SPDK's high-performance user-space storage stack with native async/await syntax. It provides:

- **Async/await I/O** - No manual callback management
- **Runtime-agnostic** - Works with Tokio, smol, async-executor, or any local executor
- **Block device API** - `Bdev`, `BdevDesc` for bdev read/write
- **Direct NVMe API** - `NvmeController`, `NvmeQpair` for low-level NVMe access
- **Multi-core I/O** - `SpdkEvent` for dispatching work across reactor cores
- **Safe wrappers** - RAII resource management, Rust safety over raw FFI

## Workspace Structure

```
spdk-io/
├── spdk-io/           # High-level async Rust API
├── spdk-io-sys/       # Low-level FFI bindings (bindgen)
├── pkgconf/           # Build helper for pkg-config parsing
└── docs/design/       # Design documentation
```

## Prerequisites

SPDK must be installed and accessible via `pkg-config`. The library links statically against SPDK.

### Install SPDK (Ubuntu/Debian)

```bash
# Option 1: Use pre-built package (if available)
sudo dpkg -i spdk_26.01-1_amd64.deb

# Option 2: Build from source
git clone https://github.com/spdk/spdk.git --recursive
cd spdk
./scripts/pkgdep.sh
./configure --with-shared --prefix=/opt/spdk
make -j$(nproc)
sudo make install
```

Ensure `PKG_CONFIG_PATH` includes SPDK:
```bash
export PKG_CONFIG_PATH=/opt/spdk/lib/pkgconfig:$PKG_CONFIG_PATH
```

## Building

```bash
cargo build
```

## Testing

Tests can run without hugepages using `no_huge` mode:

```bash
# Run all tests (requires root or appropriate capabilities)
sudo -E cargo test

# Run specific test
sudo -E cargo test --package spdk-io --test app_test
```

For NVMf tests, start the target first:
```bash
# Terminal 1: Start NVMf target
sudo /opt/spdk/bin/nvmf_tgt -m 0x4

# Terminal 2: Run tests
sudo -E cargo test --package spdk-io --test nvmf_test
```

## Quick Example

```rust
use spdk_io::{SpdkApp, Bdev, DmaBuf};

fn main() {
    SpdkApp::builder()
        .name("example")
        .json_data(r#"{
            "subsystems": [{
                "subsystem": "bdev",
                "config": [{
                    "method": "bdev_null_create",
                    "params": {"name": "Null0", "num_blocks": 1024, "block_size": 512}
                }]
            }]
        }"#)
        .run_async(|| async {
            let bdev = Bdev::get_by_name("Null0").unwrap();
            let desc = bdev.open(true).unwrap();
            let channel = desc.get_io_channel().unwrap();

            let mut buf = DmaBuf::alloc(512, 512).unwrap();
            buf.as_mut_slice().fill(0xAB);

            desc.write(&channel, &buf, 0, 512).await.unwrap();
            desc.read(&channel, &mut buf, 0, 512).await.unwrap();

            SpdkApp::stop();
        })
        .unwrap();
}
```

## Documentation

See [docs/design/](docs/design/README.md) for detailed design documentation:

- [Implementation Status](docs/design/status.md)
- [Core Types](docs/design/core-types.md) - SpdkApp, SpdkThread, SpdkEvent
- [NVMe API](docs/design/nvme.md) - Direct NVMe driver access
- [Memory & Thread Safety](docs/design/memory.md) - Send/Sync considerations

## License

MIT
