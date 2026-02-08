//! Integration test for SPDK environment initialization
//!
//! Each test in tests/ runs in its own process, which is required
//! because SPDK can only be initialized once per process.

use spdk_io::{Result, SpdkEnv};

#[test]
fn test_env_init_vdev() -> Result<()> {
    // Use no_huge (vdev mode) to run without hugepage configuration
    // mem_size_mb is required when no_huge is set
    let env = SpdkEnv::builder()
        .name("test_vdev")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(64)
        .log_level(spdk_io::LogLevel::Debug)
        .build()?;

    assert!(SpdkEnv::is_initialized());

    drop(env);

    // Note: Can't re-init after drop (DPDK limitation)
    assert!(!SpdkEnv::is_initialized());

    Ok(())
}
