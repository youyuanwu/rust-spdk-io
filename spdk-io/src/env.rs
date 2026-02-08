//! SPDK environment initialization
//!
//! The SPDK environment must be initialized once per process before using
//! any other SPDK functionality. This module provides [`SpdkEnv`] which
//! manages the environment lifetime.
//!
//! # Example
//!
//! ```no_run
//! use spdk_io::SpdkEnv;
//!
//! fn main() {
//!     let _env = SpdkEnv::builder()
//!         .name("my_app")
//!         .build()
//!         .expect("Failed to initialize SPDK");
//!
//!     // Use SPDK...
//!     
//! } // SpdkEnv dropped here, SPDK cleaned up
//! ```

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};

use spdk_io_sys::*;

use crate::error::{Error, Result};

/// SPDK log level for controlling verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum LogLevel {
    /// Disable all logging
    Disabled = spdk_log_level_SPDK_LOG_DISABLED,
    /// Error messages only
    Error = spdk_log_level_SPDK_LOG_ERROR,
    /// Warnings and errors
    Warn = spdk_log_level_SPDK_LOG_WARN,
    /// Notices, warnings, and errors (default)
    Notice = spdk_log_level_SPDK_LOG_NOTICE,
    /// Info, notices, warnings, and errors
    Info = spdk_log_level_SPDK_LOG_INFO,
    /// Debug - all messages (very verbose)
    Debug = spdk_log_level_SPDK_LOG_DEBUG,
}

/// Global flag to track if SPDK environment is initialized
static ENV_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// SPDK environment guard.
///
/// Initialized once per process. When dropped, SPDK is cleaned up and
/// **cannot be re-initialized** (DPDK limitation).
///
/// # Privilege Requirements
///
/// SPDK typically requires elevated privileges for:
/// - Hugepage access
/// - PCI device access (VFIO/UIO)
/// - Memory locking (`mlockall`)
///
/// See the design document for options to run without root.
pub struct SpdkEnv {
    _private: (),
}

impl SpdkEnv {
    /// Create a builder for configuring the SPDK environment.
    pub fn builder() -> SpdkEnvBuilder {
        SpdkEnvBuilder::new()
    }

    /// Initialize with default options.
    ///
    /// Equivalent to `SpdkEnv::builder().build()`.
    pub fn init() -> Result<Self> {
        Self::builder().build()
    }

    /// Check if the SPDK environment is currently initialized.
    pub fn is_initialized() -> bool {
        ENV_INITIALIZED.load(Ordering::SeqCst)
    }
}

impl Drop for SpdkEnv {
    fn drop(&mut self) {
        // Clean up SPDK environment
        // WARNING: After this, SPDK cannot be re-initialized in this process
        unsafe {
            spdk_env_fini();
        }
        ENV_INITIALIZED.store(false, Ordering::SeqCst);
    }
}

/// Builder for configuring SPDK environment initialization.
pub struct SpdkEnvBuilder {
    name: Option<String>,
    core_mask: Option<String>,
    mem_size_mb: Option<i32>,
    shm_id: Option<i32>,
    no_pci: bool,
    no_huge: bool,
    hugepage_single_segments: bool,
    main_core: Option<i32>,
    log_level: Option<LogLevel>,
}

impl SpdkEnvBuilder {
    /// Create a new builder with default options.
    pub fn new() -> Self {
        Self {
            name: None,
            core_mask: None,
            mem_size_mb: None,
            shm_id: None,
            no_pci: false,
            no_huge: false,
            hugepage_single_segments: false,
            main_core: None,
            log_level: None,
        }
    }

    /// Set the application name (used in hugepage file names).
    pub fn name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    /// Set the CPU core mask (e.g., "0x3" for cores 0 and 1).
    pub fn core_mask(mut self, mask: &str) -> Self {
        self.core_mask = Some(mask.to_string());
        self
    }

    /// Set the amount of hugepage memory to reserve in MB.
    pub fn mem_size_mb(mut self, mb: i32) -> Self {
        self.mem_size_mb = Some(mb);
        self
    }

    /// Set the shared memory ID for multi-process mode.
    ///
    /// Use -1 to disable shared memory (single process).
    pub fn shm_id(mut self, id: i32) -> Self {
        self.shm_id = Some(id);
        self
    }

    /// Disable PCI device scanning.
    ///
    /// Useful for testing with malloc bdevs only.
    pub fn no_pci(mut self, no_pci: bool) -> Self {
        self.no_pci = no_pci;
        self
    }

    /// Disable hugepage allocation (use regular memory).
    ///
    /// Useful for testing without configuring hugepages.
    /// Note: Performance will be reduced compared to hugepages.
    pub fn no_huge(mut self, no_huge: bool) -> Self {
        self.no_huge = no_huge;
        self
    }

    /// Use single-file hugepages.
    pub fn hugepage_single_segments(mut self, single: bool) -> Self {
        self.hugepage_single_segments = single;
        self
    }

    /// Set the main (first) core to use.
    pub fn main_core(mut self, core: i32) -> Self {
        self.main_core = Some(core);
        self
    }

    /// Set the log level for SPDK messages printed to stderr.
    ///
    /// Use [`LogLevel::Debug`] for verbose output during development.
    /// Default is [`LogLevel::Notice`].
    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = Some(level);
        self
    }

    /// Initialize the SPDK environment with the configured options.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - SPDK is already initialized
    /// - Hugepage allocation fails
    /// - PCI access fails
    /// - Other DPDK/SPDK initialization failures
    pub fn build(self) -> Result<SpdkEnv> {
        // Check if already initialized
        if ENV_INITIALIZED.swap(true, Ordering::SeqCst) {
            return Err(Error::AlreadyInitialized);
        }

        // Convert strings to CStrings
        let name_cstr = self.name.as_deref().map(CString::new).transpose()?;
        let core_mask_cstr = self.core_mask.as_deref().map(CString::new).transpose()?;

        unsafe {
            // Initialize opts with defaults
            let mut opts: spdk_env_opts = std::mem::zeroed();
            spdk_env_opts_init(&mut opts);

            // Set opts_size for ABI compatibility
            opts.opts_size = std::mem::size_of::<spdk_env_opts>();

            // Apply our configuration
            if let Some(ref name) = name_cstr {
                opts.name = name.as_ptr();
            }
            if let Some(ref mask) = core_mask_cstr {
                opts.core_mask = mask.as_ptr();
            }
            if let Some(mem_size) = self.mem_size_mb {
                opts.mem_size = mem_size;
            }
            if let Some(shm_id) = self.shm_id {
                opts.shm_id = shm_id;
            }
            if let Some(main_core) = self.main_core {
                opts.main_core = main_core;
            }
            opts.no_pci = self.no_pci;
            opts.no_huge = self.no_huge;
            opts.hugepage_single_segments = self.hugepage_single_segments;

            // Set log level before init if requested
            if let Some(level) = self.log_level {
                spdk_log_set_print_level(level as i32);
            }

            // Initialize SPDK environment
            let rc = spdk_env_init(&opts);
            if rc != 0 {
                ENV_INITIALIZED.store(false, Ordering::SeqCst);
                return Err(Error::EnvInit(format!(
                    "spdk_env_init failed with error code {}",
                    rc
                )));
            }
        }

        Ok(SpdkEnv { _private: () })
    }
}

impl Default for SpdkEnvBuilder {
    fn default() -> Self {
        Self::new()
    }
}
