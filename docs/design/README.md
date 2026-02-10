# spdk-io Design Documentation

High-level async Rust bindings for SPDK (Storage Performance Development Kit).

## Overview

`spdk-io` enables Rust applications to leverage SPDK's high-performance user-space storage stack with native async/await syntax.

## Documentation Index

| Document | Description |
|----------|-------------|
| [Implementation Status](status.md) | What's complete, in progress, and planned |
| [Crate Structure](crate-structure.md) | Package organization and build/linking |
| [Runtime Architecture](architecture.md) | Threading model, async integration, poller |
| [Core Types](core-types.md) | SpdkEnv, SpdkApp, SpdkThread, IoChannel, DmaBuf |
| [Block Device API](bdev.md) | Bdev, BdevDesc, async read/write |
| [NVMe API](nvme.md) | Direct NVMe driver access |
| [NVMf API](nvmf.md) | NVMe-oF target for testing |
| [Memory & Thread Safety](memory.md) | DMA buffers, Send/Sync, resource cleanup |
| [Testing](testing.md) | Virtual bdevs, unit tests, integration tests |
| [Future & References](future.md) | Planned features, dependencies, references |

## Quick Start

```rust
use spdk_io::{SpdkApp, Result};

fn main() -> Result<()> {
    SpdkApp::builder()
        .name("my_app")
        .config_file("./config.json")
        .run(|| {
            println!("SPDK is running!");
            SpdkApp::stop();
        })
}
```

## Key Design Principles

1. **Runtime-agnostic** - Works with any single-threaded async executor
2. **Async/await for I/O** - No manual callback management
3. **Thread-local I/O channels** - Lock-free I/O submission
4. **RAII resource management** - Automatic cleanup via Drop
5. **Safe wrappers** - Rust safety over raw FFI

## Architecture Diagram

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
