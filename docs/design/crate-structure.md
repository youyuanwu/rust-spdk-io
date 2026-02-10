# Crate Structure

## Package Layout

```
spdk-io/
├── spdk-io-build/        # Build helper crate
│   └── src/lib.rs        # PkgConfigParser with force_whole_archive
│
├── spdk-io-sys/          # Low-level FFI bindings
│   ├── build.rs          # Bindgen + linking with force_whole_archive for subsystems
│   ├── src/
│   │   └── lib.rs        # Generated bindings + manual additions
│   └── wrapper.h         # SPDK headers to bind
│
└── spdk-io/              # High-level async Rust API
    ├── src/
    │   ├── lib.rs
    │   ├── app.rs        # SpdkApp/SpdkAppBuilder (spdk_app_start framework)
    │   ├── bdev.rs       # Bdev/BdevDesc block device API with async read/write
    │   ├── channel.rs    # I/O channel management
    │   ├── complete.rs   # Callback-to-future utilities, block_on helper
    │   ├── dma.rs        # DMA buffer management
    │   ├── env.rs        # SpdkEnv/SpdkEnvBuilder (low-level env init)
    │   ├── error.rs      # Error types
    │   ├── poller.rs     # SPDK poller task for executor integration
    │   ├── thread.rs     # SPDK thread management
    │   ├── nvme/         # NVMe driver API
    │   │   ├── mod.rs
    │   │   ├── controller.rs
    │   │   ├── namespace.rs
    │   │   ├── qpair.rs
    │   │   └── transport.rs
    │   └── nvmf/         # NVMf target API
    │       ├── mod.rs
    │       ├── target.rs
    │       ├── subsystem.rs
    │       ├── transport.rs
    │       └── opts.rs
    ├── tests/
    │   ├── app_test.rs   # SpdkApp simple test
    │   ├── bdev_test.rs  # Bdev/BdevDesc with null bdev
    │   ├── env_init.rs   # SpdkEnv initialization test
    │   ├── mempool_test.rs
    │   ├── nvmf_test.rs  # NVMf subprocess integration test
    │   └── thread_test.rs
    └── Cargo.toml
```

## spdk-io-sys

Low-level FFI bindings crate:

- **Generated via bindgen** from SPDK headers
- **Links to SPDK** static libraries via pkg-config or explicit paths
- **Exports raw types**: `spdk_bdev`, `spdk_blob`, `spdk_io_channel`, etc.
- **Exports raw functions**: `spdk_bdev_read()`, `spdk_blob_io_write()`, etc.
- **Minimal safe wrappers**: Only for ergonomics (e.g., `Default` impls)

## spdk-io

High-level async Rust crate:

- **Safe wrappers** around `spdk-io-sys` types
- **Async/await API** for all I/O operations
- **Runtime-agnostic** - works with any local executor (Tokio, async-std, smol, etc.)
- **RAII resource management** (Drop implementations)
- **Error handling** via `Result<T, SpdkError>`
- **Uses `futures` ecosystem** - `futures-util`, `futures-channel` for portability

## Build & Linking

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
