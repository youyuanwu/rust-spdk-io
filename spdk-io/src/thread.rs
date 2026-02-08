//! SPDK thread management
//!
//! An [`SpdkThread`] is SPDK's lightweight scheduling context (similar to a green thread).
//! It is NOT an OS thread - it runs on whatever OS thread calls [`SpdkThread::poll()`].
//!
//! Each OS thread that performs SPDK I/O needs an `SpdkThread` attached to it.
//! The thread provides:
//! - Message passing between SPDK threads
//! - Poller scheduling
//! - I/O channel allocation
//!
//! # Example
//!
//! ```no_run
//! use spdk_io::{SpdkEnv, SpdkThread};
//!
//! fn main() {
//!     let _env = SpdkEnv::builder()
//!         .name("app")
//!         .no_pci(true)
//!         .no_huge(true)
//!         .mem_size_mb(64)
//!         .build()
//!         .expect("Failed to init SPDK");
//!
//!     // Create and attach thread to current OS thread
//!     let thread = SpdkThread::new("worker").expect("Failed to create thread");
//!
//!     // Poll in a loop (typically in an async task)
//!     loop {
//!         let work_done = thread.poll();
//!         if work_done == 0 {
//!             // Yield to other tasks...
//!             break; // For example only
//!         }
//!     }
//! }
//! ```

use std::ffi::CString;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use spdk_io_sys::*;

use crate::error::{Error, Result};

/// Global flag to track if thread library is initialized
static THREAD_LIB_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Default message mempool size (same as SPDK's SPDK_DEFAULT_MSG_MEMPOOL_SIZE)
pub const DEFAULT_MSG_MEMPOOL_SIZE: usize = 262144 - 1;

/// Smaller mempool size for testing (1023 entries)
pub const SMALL_MSG_MEMPOOL_SIZE: usize = 1023;

/// Initialize the SPDK thread library with custom mempool size.
///
/// This is called automatically when creating the first [`SpdkThread`].
/// Can also be called explicitly for custom initialization.
///
/// # Arguments
///
/// * `msg_mempool_size` - Size of the message mempool. Use [`SMALL_MSG_MEMPOOL_SIZE`]
///   for testing without hugepages, or [`DEFAULT_MSG_MEMPOOL_SIZE`] for production.
///
/// # Errors
///
/// Returns an error if initialization fails.
pub fn thread_lib_init_ext(msg_mempool_size: usize) -> Result<()> {
    if THREAD_LIB_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(()); // Already initialized
    }

    let rc = unsafe { spdk_thread_lib_init_ext(None, None, 0, msg_mempool_size) };
    if rc != 0 {
        THREAD_LIB_INITIALIZED.store(false, Ordering::SeqCst);
        return Err(Error::EnvInit(format!(
            "spdk_thread_lib_init_ext failed with error code {}",
            rc
        )));
    }

    Ok(())
}

/// Initialize the SPDK thread library with default settings (simple version).
///
/// This uses `spdk_thread_lib_init` which doesn't require a custom mempool size.
/// It's simpler and works better with the standard SPDK setup.
pub fn thread_lib_init() -> Result<()> {
    if THREAD_LIB_INITIALIZED.swap(true, Ordering::SeqCst) {
        return Ok(()); // Already initialized
    }

    // Use the simple init function - it uses default mempool settings
    let rc = unsafe { spdk_thread_lib_init(None, 0) };
    if rc != 0 {
        THREAD_LIB_INITIALIZED.store(false, Ordering::SeqCst);
        return Err(Error::EnvInit(format!(
            "spdk_thread_lib_init failed with error code {}",
            rc
        )));
    }

    Ok(())
}

/// Finalize the SPDK thread library.
///
/// Called automatically when the last thread is destroyed.
/// All threads must be destroyed before calling this.
fn thread_lib_fini() {
    if THREAD_LIB_INITIALIZED.swap(false, Ordering::SeqCst) {
        unsafe {
            spdk_thread_lib_fini();
        }
    }
}

/// SPDK thread context.
///
/// This is a lightweight scheduling context, not an OS thread. It must be
/// polled on the OS thread that created it.
///
/// # Thread Safety
///
/// `SpdkThread` is `!Send` and `!Sync` - it must stay on the OS thread
/// that created it. This is enforced at compile time.
pub struct SpdkThread {
    ptr: NonNull<spdk_thread>,
    /// Prevent Send/Sync - thread must stay on creating OS thread
    _marker: PhantomData<*mut ()>,
}

impl SpdkThread {
    /// Attach an SPDK thread context to the CURRENT OS thread.
    ///
    /// This does NOT create a new OS thread - you're already on one.
    /// An SPDK thread is a lightweight scheduling context (like a green thread),
    /// not a real OS thread. It provides:
    /// - Message passing between SPDK threads
    /// - Poller scheduling  
    /// - I/O channel allocation
    ///
    /// The thread library is initialized automatically if needed.
    ///
    /// # Arguments
    ///
    /// * `name` - Thread name (for debugging/logging)
    ///
    /// # Errors
    ///
    /// Returns an error if thread creation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use spdk_io::SpdkThread;
    ///
    /// // On your existing OS thread, attach an SPDK context
    /// let thread = SpdkThread::current("worker").unwrap();
    ///
    /// // Now poll it in your event loop
    /// loop {
    ///     thread.poll();
    ///     // ... do other work ...
    ///     # break;
    /// }
    /// ```
    pub fn current(name: &str) -> Result<Self> {
        // Use simple init (no custom mempool size)
        thread_lib_init()?;

        let name_cstr = CString::new(name)?;

        let ptr = unsafe { spdk_thread_create(name_cstr.as_ptr(), std::ptr::null()) };

        let ptr = NonNull::new(ptr)
            .ok_or_else(|| Error::EnvInit("spdk_thread_create returned NULL".to_string()))?;

        // Set as current thread for this OS thread
        unsafe {
            spdk_set_thread(ptr.as_ptr());
        }

        Ok(Self {
            ptr,
            _marker: PhantomData,
        })
    }

    /// Alias for [`current`](Self::current) - creates an SPDK thread on current OS thread.
    #[inline]
    pub fn new(name: &str) -> Result<Self> {
        Self::current(name)
    }

    /// Attach an SPDK thread context to the current OS thread with custom mempool size.
    ///
    /// Use [`SMALL_MSG_MEMPOOL_SIZE`] for testing without hugepages.
    /// This uses `spdk_thread_lib_init_ext` which allows custom mempool sizes.
    ///
    /// # Arguments
    ///
    /// * `name` - Thread name (for debugging/logging)
    /// * `msg_mempool_size` - Size of message mempool (only used if thread lib not yet initialized)
    ///
    /// # Errors
    ///
    /// Returns an error if thread creation fails.
    pub fn current_with_mempool_size(name: &str, msg_mempool_size: usize) -> Result<Self> {
        // Initialize thread library if needed (with custom mempool size)
        thread_lib_init_ext(msg_mempool_size)?;

        let name_cstr = CString::new(name)?;

        let ptr = unsafe { spdk_thread_create(name_cstr.as_ptr(), std::ptr::null()) };

        let ptr = NonNull::new(ptr)
            .ok_or_else(|| Error::EnvInit("spdk_thread_create returned NULL".to_string()))?;

        // Set as current thread for this OS thread
        unsafe {
            spdk_set_thread(ptr.as_ptr());
        }

        Ok(Self {
            ptr,
            _marker: PhantomData,
        })
    }

    /// Alias for [`current_with_mempool_size`](Self::current_with_mempool_size).
    #[inline]
    pub fn new_with_mempool_size(name: &str, msg_mempool_size: usize) -> Result<Self> {
        Self::current_with_mempool_size(name, msg_mempool_size)
    }

    /// Get the SPDK thread currently attached to this OS thread.
    ///
    /// Returns `None` if no thread is attached.
    pub fn get_current() -> Option<CurrentThread> {
        let ptr = unsafe { spdk_get_thread() };
        NonNull::new(ptr).map(|ptr| CurrentThread {
            ptr,
            _marker: PhantomData,
        })
    }

    /// Get the app thread (first thread created).
    ///
    /// Returns `None` if no threads have been created.
    pub fn app_thread() -> Option<CurrentThread> {
        let ptr = unsafe { spdk_thread_get_app_thread() };
        NonNull::new(ptr).map(|ptr| CurrentThread {
            ptr,
            _marker: PhantomData,
        })
    }

    /// Poll the thread to process messages and run pollers.
    ///
    /// Returns the number of events processed. If 0, consider yielding
    /// to other tasks before polling again.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use spdk_io::SpdkThread;
    /// # fn example(thread: &SpdkThread) {
    /// loop {
    ///     let work = thread.poll();
    ///     if work == 0 {
    ///         // Yield to async runtime
    ///         std::thread::yield_now();
    ///     }
    /// }
    /// # }
    /// ```
    pub fn poll(&self) -> i32 {
        unsafe { spdk_thread_poll(self.ptr.as_ptr(), 0, 0) }
    }

    /// Poll with a maximum number of messages to process.
    ///
    /// # Arguments
    ///
    /// * `max_msgs` - Maximum messages to process (0 = unlimited)
    pub fn poll_max(&self, max_msgs: u32) -> i32 {
        unsafe { spdk_thread_poll(self.ptr.as_ptr(), max_msgs, 0) }
    }

    /// Check if the thread has active pollers.
    pub fn has_active_pollers(&self) -> bool {
        unsafe { spdk_thread_has_active_pollers(self.ptr.as_ptr()) != 0 }
    }

    /// Check if the thread has any pollers (active or timed).
    pub fn has_pollers(&self) -> bool {
        unsafe { spdk_thread_has_pollers(self.ptr.as_ptr()) }
    }

    /// Check if the thread is idle (no work pending).
    pub fn is_idle(&self) -> bool {
        unsafe { spdk_thread_is_idle(self.ptr.as_ptr()) }
    }

    /// Check if the thread is running (not exited).
    pub fn is_running(&self) -> bool {
        unsafe { spdk_thread_is_running(self.ptr.as_ptr()) }
    }

    /// Get the thread name.
    pub fn name(&self) -> &str {
        unsafe {
            let ptr = spdk_thread_get_name(self.ptr.as_ptr());
            if ptr.is_null() {
                ""
            } else {
                std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("")
            }
        }
    }

    /// Get the thread ID.
    pub fn id(&self) -> u64 {
        unsafe { spdk_thread_get_id(self.ptr.as_ptr()) }
    }

    /// Get the total number of SPDK threads.
    pub fn count() -> u32 {
        unsafe { spdk_thread_get_count() }
    }

    /// Get the raw pointer to the underlying `spdk_thread`.
    ///
    /// # Safety
    ///
    /// The caller must ensure the pointer is not used after the thread is dropped.
    pub fn as_ptr(&self) -> *mut spdk_thread {
        self.ptr.as_ptr()
    }
}

impl Drop for SpdkThread {
    fn drop(&mut self) {
        unsafe {
            // Request thread exit
            spdk_thread_exit(self.ptr.as_ptr());

            // Poll until exited
            while !spdk_thread_is_exited(self.ptr.as_ptr()) {
                spdk_thread_poll(self.ptr.as_ptr(), 0, 0);
            }

            // Clear current thread
            spdk_set_thread(std::ptr::null_mut());

            // Destroy the thread
            spdk_thread_destroy(self.ptr.as_ptr());
        }

        // If this was the last thread, finalize the library
        if Self::count() == 0 {
            thread_lib_fini();
        }
    }
}

/// A borrowed reference to the current SPDK thread.
///
/// This is returned by [`SpdkThread::get_current()`] and does not own the thread.
/// It cannot be used to destroy the thread.
pub struct CurrentThread {
    ptr: NonNull<spdk_thread>,
    _marker: PhantomData<*mut ()>,
}

impl CurrentThread {
    /// Poll the thread.
    pub fn poll(&self) -> i32 {
        unsafe { spdk_thread_poll(self.ptr.as_ptr(), 0, 0) }
    }

    /// Get the thread name.
    pub fn name(&self) -> &str {
        unsafe {
            let ptr = spdk_thread_get_name(self.ptr.as_ptr());
            if ptr.is_null() {
                ""
            } else {
                std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("")
            }
        }
    }

    /// Get the thread ID.
    pub fn id(&self) -> u64 {
        unsafe { spdk_thread_get_id(self.ptr.as_ptr()) }
    }

    /// Get the raw pointer.
    pub fn as_ptr(&self) -> *mut spdk_thread {
        self.ptr.as_ptr()
    }
}
