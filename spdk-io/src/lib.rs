//! High-level async Rust bindings for SPDK (Storage Performance Development Kit)
//!
//! This crate provides safe, ergonomic, async-first bindings to SPDK's
//! high-performance user-space storage stack.
//!
//! # Quick Start
//!
//! ```no_run
//! use spdk_io::{SpdkEnv, Result};
//!
//! fn main() -> Result<()> {
//!     // Initialize SPDK environment (once per process)
//!     let _env = SpdkEnv::builder()
//!         .name("my_app")
//!         .build()?;
//!
//!     // Use SPDK...
//!
//!     Ok(())
//! }
//! ```
//!
//! # Modules
//!
//! - [`env`] - Environment initialization
//! - [`thread`] - SPDK thread management
//! - [`error`] - Error types

pub mod env;
pub mod error;
pub mod thread;

// Re-exports
pub use env::{LogLevel, SpdkEnv, SpdkEnvBuilder};
pub use error::{Error, Result};
pub use thread::{CurrentThread, SpdkThread};
