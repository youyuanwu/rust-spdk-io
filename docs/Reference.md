# SPDK

## Reference projects

- [spdk-rs](https://github.com/openebs/spdk-rs)
  - **Summary**: Higher-level Rust bindings and wrappers around SPDK for building safer SPDK-based Rust applications. Most actively maintained project (85 stars, 16 contributors). Uses Nix package manager for reproducible development environment. Developed by OpenEBS for Mayastor storage products. Supports SPDK v24.01+ with static library linking. Provides helper scripts for SPDK configuration/compilation.
  - **FFI Bindings**: Uses bindgen at build time with `wrapper.h` including many SPDK headers. Generated code stored in `src/libspdk/mod.rs` (includes from `OUT_DIR`). Extensive `build.rs` with library discovery via pkg-config, allowlist/blocklist for types/functions. Also compiles C helpers in `helpers/` directory.
  - **Rust API Layout**:
    - Generic wrapper types with `BdevData` type parameter: `Bdev<BdevData>`, `BdevDesc<BdevData>`, `BdevIo<BdevData>`, `IoChannel<ChannelData>`
    - Uses `NonNull<T>` for safe pointer wrappers, lifetimes for module references
    - Builder pattern: `BdevBuilder`, `BdevModuleBuilder`, `PollerBuilder`
    - Modules: `bdev`, `bdev_module`, `io_channel`, `thread`, `poller`, `dma`, `nvme`, `nvmf`
    - Helper utilities in `ffihelper.rs`: `cb_arg()`, `done_cb()`, `ErrnoResult`, string conversions
    - Async support via `futures::channel::oneshot` for callback-to-async conversion
  - **Multi-threading**:
    - `Thread::new(name, core)` - Create SPDK thread pinned to specific CPU core via `CpuMask`
    - `Thread::send_msg(args, f)` - Cross-thread messaging via `spdk_thread_send_msg()`
    - `Thread::with(f)` - Execute closure with thread as current context
    - `CurrentThreadGuard` - RAII guard for saving/restoring current SPDK thread
    - `PollerBuilder::with_core(core)` - Run poller on different core (creates thread, sends msg to register)
    - `Cores` / `CoreIterator` - Iterate over available CPU cores
    - `CpuMask` - CPU affinity mask wrapper
    - `RoundRobinCoreSelector` - Load balancing across cores
    - `IoDeviceChannelTraverse` - Async iteration over all I/O channels via `spdk_for_each_channel`

- [starfish](https://github.com/jkozlowski/starfish) Rust futures on spdk
  - **Summary**: Async programming framework with SPDK for Rust (Linux only). Provides a futures executor (`starfish-executor`) that integrates with SPDK's event loop. Includes `spdk-sys` for low-level bindings. Last updated 7 years ago (2018), built for SPDK v18.07.1. 33 stars.
  - **FFI Bindings**: Separate `spdk-sys` crate. Uses bindgen to generate from SPDK headers (nvme, event, bdev, env, blob, blob_bdev, io_channel). Expects SPDK installed at `/usr/local/lib`. Links against `libspdk.so` shared library.
  - **Rust API Layout**:
    - Custom single-threaded executor in `starfish-executor`: `CurrentThreadExecutor`, `TaskQueue`, `TaskHandle`
    - Custom waker implementation: `RcWake` trait, `waker_vtable!` macro
    - Blob API: `Blobstore`, `Blob`, `BlobId`, `IoChannel`, `Buf` (DMA buffer)
    - Async functions using `oneshot::channel` callback pattern: `bs_init()`, `create()`, `open()`, `read()`, `write()`, `close()`
    - Event loop integration via `poller_register()` and `pure_poll()` entry point
    - `AppOpts` for SPDK application initialization
  - **Multi-threading**: Single-threaded executor design, no multi-core support

- [async-spdk](https://github.com/madsys-dev/async-spdk)
  - **Summary**: Asynchronous Rust bindings for SPDK from MadSys research group. Provides Blob and BlobFs APIs with async/await support. Includes hello_blob and hello_bdev examples. Requires root privileges. Last updated 4 years ago. 17 stars, 2 contributors.
  - **FFI Bindings**: Separate `spdk-sys` crate. Builds SPDK from source as git submodule, creates fat shared library `libspdk_fat.so` by linking all static libs together. Uses bindgen with `wrapper.h` (bdev, blob, blob_bdev, env, event, blobfs, vmd, log).
  - **Rust API Layout**:
    - Both sync and async APIs with `impl SpdkFile` split into `/// Sync API` and `/// Async API` blocks
    - Main entry: `AppOpts` with `block_on<F: Future>()` method for running async code
    - `spawn()` function + `JoinHandle<F>` for spawning futures, uses `spdk_poller_register`
    - `LocalComplete<T>` type + `do_async()` helper for callback-to-async conversion
    - Blob API: `Blobstore`, `Blob`, `BlobId`, `IoChannel`
    - BlobFs API: `SpdkFilesystem`, `SpdkFile`, `SpdkFsThreadCtx`, `SpdkBlobfsOpts`
    - Bdev API: `BDev`, `BdevDesc`, `BdevIo`, `DmaBuf`
    - Thread/Poller management: `Thread`, `Poller`, `CpuSet`
  - **Multi-threading**:
    - `AppOpts::reactor_mask(mask)` - Set CPU cores for SPDK reactors (e.g., `"0x3"` for cores 0+1)
    - `AppOpts::main_core(core)` - Set main reactor core
    - `Thread::create(name, cpumask)` - Create SPDK thread with CPU affinity
    - `Thread::set()` - Force current OS thread to act as SPDK thread
    - `Thread::poll(max_msgs)` - Poll thread for messages and pollers
    - `SpdkEvent::alloc(lcore, arg1, arg2)` - Allocate event for specific lcore via `spdk_event_allocate`
    - `SpdkEvent::call()` - Dispatch event to target reactor's event ring
    - `CpuSet` - CPU mask for thread affinity
    - `Poller::register(f)` / `pause()` / `resume()` - Poller management
    - Uses `std::thread::spawn` for separate OS thread in blobfs example

- [rust-spdk](https://github.com/PumpkinDB/rust-spdk)
  - **Summary**: Early/basic Rust bindings for SPDK from the PumpkinDB project. Focused on NVMe controller access from Rust. Last updated 9 years ago (2017), likely abandoned. 19 stars. Minimal wrapper approach.
  - **FFI Bindings**: Manual bindgen command documented in `HACKING.md`. Pre-generated bindings stored in `src/clib.rs`. Builds SPDK/DPDK from source in `build.rs`. Links against static libraries.
  - **Rust API Layout**:
    - NVMe-focused only, no Blob/Bdev abstractions
    - Thin newtype wrappers: `Controller(*mut spdk_nvme_ctrlr)`, `Namespace(*mut spdk_nvme_ns)`, `QueuePair(*mut spdk_nvme_qpair)`
    - Transport: `TransportIdentifier`, `OwnedTransportIdentifier` with parsing
    - Callback traits: `CommandCallback` (for I/O completion), `ProbeCallback` (for controller discovery)
    - `DMA<'a>` struct for DMA buffer management with lifetime
    - `EnvOpts` + `init_env()` for environment initialization
    - Macros: `ns_data!`, `ctrlr_data!` for accessing struct fields
  - **Multi-threading**: No threading abstractions, minimal env init only

---

## Features Not Yet Implemented in spdk-io

Based on analysis of the reference projects above, the following features are available in other projects but not yet implemented in spdk-io:

### High Priority (commonly used)

| Feature | Description | Reference |
|---------|-------------|-----------|
| **Blobstore API** | `Blobstore`, `Blob`, `BlobId` - key-value store on block device | async-spdk |
| **BlobFs API** | `SpdkFilesystem`, `SpdkFile` - POSIX-like filesystem on blobstore | async-spdk |
| **Poller API** | `Poller::register()`, `pause()`, `resume()` - periodic callback registration | spdk-rs, async-spdk |
| **CpuMask/CpuSet** | CPU affinity mask for pinning threads to specific cores | spdk-rs, async-spdk |
| **Thread::create(name, cpumask)** | Create SPDK thread pinned to specific CPU core | spdk-rs, async-spdk |

### Medium Priority (advanced use cases)

| Feature | Description | Reference |
|---------|-------------|-----------|
| **Custom Bdev Module** | `BdevModuleBuilder`, `BdevBuilder`, `BdevOps` trait - create virtual block devices | spdk-rs |
| **IoDevice trait** | `register_io_device()`, `unregister_io_device()` - custom I/O device registration | spdk-rs |
| **IoDeviceChannelTraverse** | `traverse_io_channels()` - iterate over all I/O channels via `spdk_for_each_channel` | spdk-rs |
| **Bdev Iterator** | `Bdev::iter_all()`, `BdevModuleIter` - enumerate all bdevs | spdk-rs |
| **Bdev Stats** | `stats_async()`, `reset_stats()` - device I/O statistics | spdk-rs |
| **LBA Range Locking** | `LbaRange`, `LbaRangeLock` - lock LBA ranges for exclusive access | spdk-rs |

### Lower Priority (specialized)

| Feature | Description | Reference |
|---------|-------------|-----------|
| **RoundRobinCoreSelector** | Load balancing across CPU cores | spdk-rs |
| **CurrentThreadGuard** | RAII guard for saving/restoring current SPDK thread context | spdk-rs |
| **Thread::spawn_unaffinitized()** | Spawn OS thread with inverse CPU affinity | spdk-rs |
| **Generic Bdev<BdevData>** | Type-parameterized bdev wrapper for custom data | spdk-rs |
| **IoVec wrapper** | `IoVec`, `AsIoVecPtr`, `AsIoVecs` traits for scatter-gather I/O | spdk-rs |
| **JsonWriteContext** | JSON serialization for config export | spdk-rs |
| **NvmfController/Subsystem** | NVMe-oF target management | spdk-rs |

### Already Implemented in spdk-io ✅

| Feature | Status |
|---------|--------|
| `SpdkApp` / `SpdkAppBuilder` | ✅ Done |
| `SpdkEvent::call_on()` / `call_on_async()` | ✅ Done |
| `Cores` / `CoreIterator` | ✅ Done |
| `SpdkThread` / `ThreadHandle` | ✅ Done |
| `NvmeController` / `NvmeQpair` / `NvmeNamespace` | ✅ Done |
| `DmaBuf` (DMA buffer allocation) | ✅ Done |
| `Bdev` / `BdevDesc` / `IoChannel` | ✅ Done |
| `TransportId` (PCIe, TCP, RDMA) | ✅ Done |
| Async I/O with `CompletionReceiver` | ✅ Done |