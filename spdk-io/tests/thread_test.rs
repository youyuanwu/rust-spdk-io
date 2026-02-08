//! Integration test for SPDK thread management
//!
//! All thread tests are in one function because SPDK can only be
//! initialized once per process.
//!
//! Uses the simple spdk_thread_lib_init which should work with default SPDK setup.

use spdk_io::{LogLevel, Result, SpdkEnv, SpdkThread};

// Test thread with hugepages (standard setup)
#[test]
fn test_thread() -> Result<()> {
    // Use hugepages with debug logging
    let _env = SpdkEnv::builder()
        .name("test_thread")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(256)
        .log_level(LogLevel::Debug) // Verbose logging
        .build()?;

    // Create a thread using simple init
    let thread = SpdkThread::new("worker")?;

    // Check basic properties
    assert_eq!(thread.name(), "worker");
    assert!(thread.id() > 0);
    assert!(thread.is_running());
    assert!(SpdkThread::count() >= 1);

    // Verify current thread is set
    let current = SpdkThread::get_current().expect("Current thread should be set");
    assert_eq!(current.id(), thread.id());

    // Poll should work (returns 0 when no work)
    let work = thread.poll();
    assert!(work >= 0);

    // Thread is idle when no pollers registered
    assert!(thread.is_idle());
    assert!(!thread.has_pollers());

    // Poll multiple times
    for _ in 0..10 {
        thread.poll();
    }

    // Poll with max_msgs limit
    let work = thread.poll_max(100);
    assert!(work >= 0);

    // Drop the thread
    drop(thread);

    // Current thread should be cleared
    assert!(SpdkThread::get_current().is_none());

    Ok(())
}
