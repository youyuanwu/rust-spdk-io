//! Build script for spdk-io-sys
//!
//! Uses pkg-config to find SPDK installation and generates Rust bindings via bindgen.
//! Links statically against SPDK/DPDK libraries with --whole-archive.
//!
//! Environment variables:
//! - `PKG_CONFIG_PATH`: Must include SPDK's pkg-config directory (e.g., /opt/spdk/lib/pkgconfig)

use std::env;
use std::path::PathBuf;

use pkgconf::PkgConfigParser;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    // Core SPDK libraries we need
    let spdk_libs = [
        "spdk_env_dpdk",
        "spdk_thread",
        "spdk_bdev",
        "spdk_blob",
        "spdk_blob_bdev",
        "spdk_nvme",
        "spdk_nvmf", // NVMe-oF target
        "spdk_log",
        "spdk_util",
        "spdk_json",
        "spdk_rpc",
        "spdk_jsonrpc",
        "spdk_event",
        "spdk_event_bdev", // Register bdev subsystem with event framework
        "spdk_event_nvmf", // Register nvmf subsystem with event framework
        "spdk_bdev_malloc",
        "spdk_bdev_null",
        "spdk_accel",      // Accel framework + software module
        "spdk_sock",       // Socket abstraction
        "spdk_sock_posix", // POSIX socket implementation
        "libdpdk",
        "spdk_syslibs", // System dependencies (isal, ssl, crypto, uuid, fuse3, aio, etc.)
    ];

    // PKG_CONFIG_PATH for SPDK installation
    let pkg_config_path =
        env::var("PKG_CONFIG_PATH").unwrap_or_else(|_| "/opt/spdk/lib/pkgconfig".to_string());

    // SPDK event subsystem libraries use SPDK_SUBSYSTEM_REGISTER() which creates
    // constructor functions. These need --whole-archive or the linker will discard them.
    // Bdev modules also use SPDK_BDEV_MODULE_REGISTER() with constructors.
    // Accel modules use SPDK_ACCEL_MODULE_REGISTER() with constructors.
    // NVMe transports use SPDK_NVME_TRANSPORT_REGISTER() with constructors.
    let parser = PkgConfigParser::new().force_whole_archive([
        "spdk_event_bdev",
        "spdk_event_nvmf",
        "spdk_event_accel",
        "spdk_event_vmd",
        "spdk_event_sock",
        "spdk_event_iobuf",
        "spdk_event_keyring",
        "spdk_bdev_null",
        "spdk_bdev_malloc",
        "spdk_accel",      // Contains software accel module (accel_sw)
        "spdk_sock_posix", // POSIX socket implementation
        "spdk_nvmf",       // NVMf target with transport registrations
        "spdk_nvme",       // NVMe initiator with transport registrations (TCP, RDMA, etc.)
    ]);

    // Single probe call: parses both --libs and --cflags
    let pkg = parser
        .probe(spdk_libs, Some(&pkg_config_path))
        .expect("pkg-config failed");

    // Emit cargo linker directives (no_bundle=true for -sys crate with `links` key)
    pkgconf::emit_cargo_metadata(&pkg.libs, true);

    // Build clang args for bindgen from parsed cflags
    let clang_args = pkgconf::to_clang_args(&pkg.cflags);

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        // Allowlist SPDK types and functions
        .allowlist_function("spdk_.*")
        .allowlist_type("spdk_.*")
        .allowlist_var("SPDK_.*")
        // Also allow some DPDK types we need
        .allowlist_type("rte_.*")
        .allowlist_function("rte_.*")
        // Generate Default impls for structs
        .derive_default(true)
        .derive_debug(true)
        .derive_copy(true)
        // Rust 2024 compatibility - wrap extern blocks in unsafe
        .wrap_unsafe_ops(true)
        // Handle opaque types (internal SPDK structs we don't need layout for)
        .opaque_type("spdk_nvme_ctrlr")
        .opaque_type("spdk_nvme_ns")
        .opaque_type("spdk_nvme_qpair")
        .opaque_type("spdk_bdev")
        .opaque_type("spdk_bdev_desc")
        .opaque_type("spdk_io_channel")
        .opaque_type("spdk_thread")
        .opaque_type("spdk_poller")
        .opaque_type("spdk_blob_store")
        .opaque_type("spdk_blob")
        // Make packed structs with aligned fields opaque to avoid E0588
        .opaque_type("spdk_nvme_ctrlr_data")
        .opaque_type("spdk_bdev_ext_io_opts")
        .opaque_type("spdk_nvmf_fabric_connect_rsp")
        .opaque_type("spdk_nvmf_fabric_prop_get_rsp")
        .opaque_type("spdk_nvme_tcp_cmd")
        .opaque_type("spdk_nvme_tcp_rsp")
        .opaque_type("spdk_nvmf_transport_opts")
        .opaque_type("spdk_nvme_cdata_oncs")
        // NVMf opaque types
        .opaque_type("spdk_nvmf_tgt")
        .opaque_type("spdk_nvmf_transport")
        .opaque_type("spdk_nvmf_subsystem")
        .opaque_type("spdk_nvmf_poll_group")
        .opaque_type("spdk_nvmf_qpair")
        .opaque_type("spdk_nvmf_ctrlr")
        .opaque_type("spdk_nvmf_ns")
        // Layout tests can fail on different systems
        .layout_tests(false)
        .generate()
        .expect("Failed to generate SPDK bindings");

    // Write bindings to OUT_DIR
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Failed to write bindings");
}
