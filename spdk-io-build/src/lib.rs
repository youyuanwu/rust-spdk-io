//! Build helper utilities for parsing pkg-config output with proper --whole-archive support.
//!
//! The standard `pkg-config` crate does not preserve the ordering of `-Wl,--whole-archive`
//! and `-l` flags, which breaks linking for libraries that use constructor functions
//! (like DPDK's RTE_INIT macros). This module provides utilities to parse pkg-config
//! output directly and emit cargo link directives in the correct order.
//!
//! # Example
//!
//! ```no_run
//! use spdk_io_build::{PkgConfigParser, COMMON_SYSTEM_LIBS};
//!
//! let parser = PkgConfigParser::new()
//!     .system_libs(COMMON_SYSTEM_LIBS);
//!
//! parser.probe_and_emit(
//!     ["spdk_env_dpdk", "spdk_thread", "libdpdk"],
//!     Some("/opt/spdk/lib/pkgconfig"),
//! ).expect("pkg-config failed");
//! ```

use std::collections::HashSet;
use std::process::Command;

/// Represents how a library should be linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Dynamic system library (e.g., pthread, numa)
    Dynamic,
    /// Static library without whole-archive
    Static,
    /// Static library with +whole-archive (includes all symbols, needed for constructors)
    WholeArchive,
}

/// A parsed linker flag from pkg-config output.
#[derive(Debug, Clone)]
pub enum LinkerFlag {
    /// Library search path (-L/path)
    SearchPath(String),
    /// Library to link (-l or -l:)
    Library { name: String, kind: LinkKind },
    /// Raw linker argument (-Wl,...)
    LinkerArg(String),
}

/// Configuration for parsing pkg-config output.
#[derive(Debug, Clone)]
pub struct PkgConfigParser {
    /// Libraries that should always be linked dynamically
    system_libs: HashSet<String>,
    /// Whether to skip emitting -bundle (for -sys crates with links key)
    no_bundle: bool,
}

impl Default for PkgConfigParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PkgConfigParser {
    /// Create a new parser with default settings.
    pub fn new() -> Self {
        Self {
            system_libs: HashSet::new(),
            no_bundle: true,
        }
    }

    /// Add a library name that should be linked dynamically (system library).
    pub fn system_lib(mut self, name: &str) -> Self {
        self.system_libs.insert(name.to_string());
        self
    }

    /// Add multiple system library names.
    pub fn system_libs<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            self.system_libs.insert(name.as_ref().to_string());
        }
        self
    }

    /// Set whether to add -bundle modifier (prevents re-export from rlib).
    /// Default is true (adds -bundle).
    pub fn no_bundle(mut self, enabled: bool) -> Self {
        self.no_bundle = enabled;
        self
    }

    /// Check if a library name is a system library.
    pub fn is_system_lib(&self, name: &str) -> bool {
        self.system_libs.contains(name)
    }

    /// Run pkg-config and get raw output.
    pub fn run_pkg_config<I, S>(
        packages: I,
        pkg_config_path: Option<&str>,
    ) -> Result<String, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let packages: Vec<String> = packages
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();

        let mut cmd = Command::new("pkg-config");

        if let Some(path) = pkg_config_path {
            cmd.env("PKG_CONFIG_PATH", path);
        }

        cmd.args(["--static", "--libs"]);
        cmd.args(&packages);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run pkg-config: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "pkg-config failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Parse pkg-config output into structured linker flags.
    ///
    /// This function tracks `--whole-archive` and `--no-whole-archive` markers
    /// in the pkg-config output and applies the appropriate LinkKind to libraries.
    /// If a library appears both outside and inside a whole-archive region,
    /// it will be upgraded to WholeArchive.
    pub fn parse(&self, pkg_config_output: &str) -> Vec<LinkerFlag> {
        let mut flags = Vec::new();
        let mut seen_libs: HashSet<String> = HashSet::new();
        // Track library indices for upgrading to WholeArchive if seen again in whole-archive region
        let mut lib_indices: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        // Track whether we're inside a --whole-archive region from pkg-config
        let mut in_whole_archive_region = false;

        for flag in pkg_config_output.split_whitespace() {
            if let Some(path) = flag.strip_prefix("-L") {
                flags.push(LinkerFlag::SearchPath(path.to_string()));
            } else if let Some(wl_args) = flag.strip_prefix("-Wl,") {
                // Handle --whole-archive/--no-whole-archive state tracking
                if wl_args.contains("--whole-archive") && !wl_args.contains("--no-whole-archive") {
                    in_whole_archive_region = true;
                } else if wl_args.contains("--no-whole-archive") {
                    in_whole_archive_region = false;
                }
                // Pass through certain linker flags
                if wl_args.contains("export-dynamic") || wl_args.contains("as-needed") {
                    flags.push(LinkerFlag::LinkerArg(flag.to_string()));
                }
                // Don't emit --whole-archive/--no-whole-archive - we handle via link-lib modifiers
            } else if let Some(rest) = flag.strip_prefix("-l:") {
                // Explicit static archive like -l:libfoo.a
                let lib_name = rest
                    .strip_prefix("lib")
                    .unwrap_or(rest)
                    .strip_suffix(".a")
                    .unwrap_or(rest);

                self.handle_library(
                    &mut flags,
                    &mut seen_libs,
                    &mut lib_indices,
                    lib_name,
                    in_whole_archive_region,
                );
            } else if let Some(lib_name) = flag.strip_prefix("-l") {
                self.handle_library(
                    &mut flags,
                    &mut seen_libs,
                    &mut lib_indices,
                    lib_name,
                    in_whole_archive_region,
                );
            } else if flag == "-pthread" && !seen_libs.contains("pthread") {
                flags.push(LinkerFlag::Library {
                    name: "pthread".to_string(),
                    kind: LinkKind::Dynamic,
                });
                seen_libs.insert("pthread".to_string());
            }
        }

        flags
    }

    /// Handle adding or upgrading a library in the flags list.
    fn handle_library(
        &self,
        flags: &mut Vec<LinkerFlag>,
        seen_libs: &mut HashSet<String>,
        lib_indices: &mut std::collections::HashMap<String, usize>,
        lib_name: &str,
        in_whole_archive_region: bool,
    ) {
        if seen_libs.contains(lib_name) {
            // Library already seen - check if we need to upgrade to WholeArchive
            if in_whole_archive_region
                && !self.is_system_lib(lib_name)
                && let Some(&idx) = lib_indices.get(lib_name)
                && let LinkerFlag::Library { kind, .. } = &mut flags[idx]
                && *kind == LinkKind::Static
            {
                *kind = LinkKind::WholeArchive;
            }
            return;
        }

        let kind = self.determine_link_kind(lib_name, in_whole_archive_region);
        let idx = flags.len();
        flags.push(LinkerFlag::Library {
            name: lib_name.to_string(),
            kind,
        });
        seen_libs.insert(lib_name.to_string());
        lib_indices.insert(lib_name.to_string(), idx);
    }

    /// Determine the link kind for a library based on system lib status and whole-archive context.
    fn determine_link_kind(&self, lib_name: &str, in_whole_archive_region: bool) -> LinkKind {
        if self.is_system_lib(lib_name) {
            LinkKind::Dynamic
        } else if in_whole_archive_region {
            // Inside --whole-archive region from pkg-config output
            LinkKind::WholeArchive
        } else {
            // Outside --whole-archive region -> regular static linking
            LinkKind::Static
        }
    }

    /// Emit cargo metadata for the parsed flags.
    pub fn emit_cargo_metadata(&self, flags: &[LinkerFlag]) {
        for flag in flags {
            match flag {
                LinkerFlag::SearchPath(path) => {
                    println!("cargo:rustc-link-search=native={}", path);
                }
                LinkerFlag::Library { name, kind } => match kind {
                    LinkKind::Dynamic => {
                        println!("cargo:rustc-link-lib={}", name);
                    }
                    LinkKind::Static => {
                        if self.no_bundle {
                            println!("cargo:rustc-link-lib=static:-bundle={}", name);
                        } else {
                            println!("cargo:rustc-link-lib=static={}", name);
                        }
                    }
                    LinkKind::WholeArchive => {
                        if self.no_bundle {
                            println!(
                                "cargo:rustc-link-lib=static:+whole-archive,-bundle={}",
                                name
                            );
                        } else {
                            println!("cargo:rustc-link-lib=static:+whole-archive={}", name);
                        }
                    }
                },
                LinkerFlag::LinkerArg(arg) => {
                    println!("cargo:rustc-link-arg={}", arg);
                }
            }
        }
    }

    /// Convenience method: run pkg-config, parse output, and emit cargo metadata.
    pub fn probe_and_emit<I, S>(
        &self,
        packages: I,
        pkg_config_path: Option<&str>,
    ) -> Result<(), String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let output = Self::run_pkg_config(packages, pkg_config_path)?;
        let flags = self.parse(&output);
        self.emit_cargo_metadata(&flags);
        Ok(())
    }

    /// Add an extra library after parsing.
    pub fn add_extra_lib(&self, name: &str, kind: LinkKind) {
        match kind {
            LinkKind::Dynamic => {
                println!("cargo:rustc-link-lib={}", name);
            }
            LinkKind::Static => {
                if self.no_bundle {
                    println!("cargo:rustc-link-lib=static:-bundle={}", name);
                } else {
                    println!("cargo:rustc-link-lib=static={}", name);
                }
            }
            LinkKind::WholeArchive => {
                if self.no_bundle {
                    println!(
                        "cargo:rustc-link-lib=static:+whole-archive,-bundle={}",
                        name
                    );
                } else {
                    println!("cargo:rustc-link-lib=static:+whole-archive={}", name);
                }
            }
        }
    }
}

/// Common system libraries used on Linux.
pub const COMMON_SYSTEM_LIBS: &[&str] = &[
    "pthread",
    "m",
    "dl",
    "numa",
    "rt",
    "crypto",
    "ssl",
    "uuid",
    "aio",
    "uring",
    "pcap",
    "ibverbs",
    "rdmacm",
    "mlx5",
    "keyutils",
    "isal",
    "isal_crypto",
    "fuse3",
    "lz4", // May need explicit versioned name (-l:liblz4.so.1) if no .so symlink
    "c",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_no_whole_archive_region() {
        let parser = PkgConfigParser::new().system_libs(["pthread", "numa"]);

        let output = "-L/opt/spdk/lib -lspdk_env -lpthread -lnuma";
        let flags = parser.parse(output);

        assert_eq!(flags.len(), 4);
        assert!(matches!(&flags[0], LinkerFlag::SearchPath(p) if p == "/opt/spdk/lib"));
        // Not in --whole-archive region -> Static
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "spdk_env" && *kind == LinkKind::Static)
        );
        assert!(
            matches!(&flags[2], LinkerFlag::Library { name, kind } if name == "pthread" && *kind == LinkKind::Dynamic)
        );
        assert!(
            matches!(&flags[3], LinkerFlag::Library { name, kind } if name == "numa" && *kind == LinkKind::Dynamic)
        );
    }

    #[test]
    fn test_parse_explicit_archive() {
        let parser = PkgConfigParser::new();

        // -l:libfoo.a without --whole-archive region -> Static
        let output = "-l:librte_mempool_ring.a";
        let flags = parser.parse(output);

        assert_eq!(flags.len(), 1);
        assert!(
            matches!(&flags[0], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::Static)
        );
    }

    #[test]
    fn test_dedup_libs() {
        let parser = PkgConfigParser::new();

        let output = "-lspdk_log -l:libspdk_log.a -lspdk_log";
        let flags = parser.parse(output);

        // Should only have one entry
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn test_whole_archive_region_from_pkgconfig() {
        // Only libs within --whole-archive region get WholeArchive
        let parser = PkgConfigParser::new();

        let output = "-lspdk_log -Wl,--whole-archive -lrte_mempool_ring -lrte_eal -Wl,--no-whole-archive -lspdk_util";
        let flags = parser.parse(output);

        assert_eq!(flags.len(), 4);
        // spdk_log before --whole-archive -> Static
        assert!(
            matches!(&flags[0], LinkerFlag::Library { name, kind } if name == "spdk_log" && *kind == LinkKind::Static)
        );
        // rte_mempool_ring inside --whole-archive region -> WholeArchive
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::WholeArchive)
        );
        // rte_eal inside --whole-archive region -> WholeArchive
        assert!(
            matches!(&flags[2], LinkerFlag::Library { name, kind } if name == "rte_eal" && *kind == LinkKind::WholeArchive)
        );
        // spdk_util after --no-whole-archive -> Static
        assert!(
            matches!(&flags[3], LinkerFlag::Library { name, kind } if name == "spdk_util" && *kind == LinkKind::Static)
        );
    }

    #[test]
    fn test_system_libs_always_dynamic() {
        let parser = PkgConfigParser::new().system_libs(["pthread"]);

        // Even inside --whole-archive region, system libs remain Dynamic
        let output = "-Wl,--whole-archive -lrte_mempool_ring -lpthread -Wl,--no-whole-archive";
        let flags = parser.parse(output);

        assert_eq!(flags.len(), 2);
        assert!(
            matches!(&flags[0], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::WholeArchive)
        );
        // System lib is Dynamic even inside --whole-archive region
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "pthread" && *kind == LinkKind::Dynamic)
        );
    }

    #[test]
    fn test_upgrade_to_whole_archive_on_duplicate() {
        // If a lib appears first outside whole-archive, then again inside,
        // it should be upgraded to WholeArchive (SPDK pkg-config does this pattern)
        let parser = PkgConfigParser::new();

        let output = "-lrte_mempool_ring -Wl,--whole-archive -l:librte_mempool_ring.a -Wl,--no-whole-archive";
        let flags = parser.parse(output);

        // Should only have one entry, but upgraded to WholeArchive
        assert_eq!(flags.len(), 1);
        assert!(
            matches!(&flags[0], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::WholeArchive)
        );
    }
}
