//! Test to check if spdk_mempool_create works

use spdk_io::{Result, SpdkEnv};
use spdk_io_sys::*;
use std::ffi::CString;

#[test]
#[ignore] // Requires hugepages
fn test_dma_malloc() -> Result<()> {
    // Initialize SPDK with hugepages - need more memory for mempools
    let _env = SpdkEnv::builder()
        .name("test_dma")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(512) // Increase memory
        .build()?;

    println!("SPDK env initialized, trying spdk_dma_malloc...");

    // Try spdk_dma_malloc
    let ptr = unsafe { spdk_dma_malloc(4096, 4096, std::ptr::null_mut()) };

    if ptr.is_null() {
        println!("spdk_dma_malloc returned NULL!");
    } else {
        println!("spdk_dma_malloc succeeded: {:p}", ptr);
        unsafe { spdk_dma_free(ptr) };
    }

    println!("Trying spdk_malloc with SPDK_MALLOC_DMA...");
    let ptr2 = unsafe { spdk_malloc(4096, 4096, std::ptr::null_mut(), -1, SPDK_MALLOC_DMA) };

    if ptr2.is_null() {
        println!("spdk_malloc with DMA returned NULL!");
    } else {
        println!("spdk_malloc with DMA succeeded: {:p}", ptr2);
        unsafe { spdk_free(ptr2) };
    }

    // Try spdk_mempool_create with different params
    println!("Trying spdk_mempool_create with tiny pool on socket 0...");
    let name = std::ffi::CString::new("tiny_pool").unwrap();
    let pool = unsafe {
        spdk_mempool_create(
            name.as_ptr(),
            8,  // count - minimum
            64, // ele_size - small
            0,  // cache_size
            0,  // socket 0 (instead of -1 for NUMA_ID_ANY)
        )
    };

    if pool.is_null() {
        println!("spdk_mempool_create with tiny pool returned NULL!");
        // Get errno
        let errno = std::io::Error::last_os_error();
        println!("Last OS error: {:?}", errno);
    } else {
        println!("spdk_mempool_create with tiny pool succeeded!");
        unsafe { spdk_mempool_free(pool) };
    }

    // Try spdk_ring_create
    println!("Trying spdk_ring_create...");
    let ring = unsafe { spdk_ring_create(spdk_ring_type_SPDK_RING_TYPE_MP_SC, 64, -1) };
    if ring.is_null() {
        println!("spdk_ring_create returned NULL!");
    } else {
        println!("spdk_ring_create succeeded!");
        unsafe { spdk_ring_free(ring) };
    }

    Ok(())
}

#[test]
fn test_mempool_create() -> Result<()> {
    // Initialize SPDK with hugepages
    let _env = SpdkEnv::builder()
        .name("test_mempool")
        .no_pci(true)
        .no_huge(true)
        .mem_size_mb(256)
        .log_level(spdk_io::LogLevel::Debug) // Verbose logging to see mempool details
        .build()?;

    println!("SPDK env initialized, trying to create mempool...");

    let name = CString::new("test_pool").unwrap();

    // Try creating a small mempool
    let pool = unsafe {
        spdk_mempool_create(
            name.as_ptr(),
            64,  // count - very small
            128, // ele_size
            0,   // cache_size
            -1,  // SPDK_ENV_NUMA_ID_ANY
        )
    };

    if pool.is_null() {
        println!("spdk_mempool_create returned NULL!");
        return Err(spdk_io::Error::MemoryAlloc);
    }

    println!("Mempool created successfully!");

    // Free the pool
    unsafe {
        spdk_mempool_free(pool);
    }

    Ok(())
}
