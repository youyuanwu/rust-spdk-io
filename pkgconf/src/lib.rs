//! Build helper utilities for parsing pkg-config output with proper `--whole-archive` support.
//!
//! # Problem
//!
//! The standard [`pkg-config`](https://crates.io/crates/pkg-config) crate does not preserve
//! the ordering of `-Wl,--whole-archive` and `-l` flags, which breaks linking for libraries
//! that use constructor functions (like DPDK's `RTE_INIT` macros). Additionally, it doesn't
//! properly distinguish between static and dynamic libraries based on file availability.
//!
//! # Solution
//!
//! This crate parses pkg-config output directly and:
//!
//! - **Tracks `--whole-archive` regions** from the pkg-config output
//! - **Auto-detects static library availability** by checking if `lib<name>.a` exists
//! - **Excludes system directories** (default: `/usr`) so system libs link dynamically
//! - **Emits correct cargo link directives** with proper modifiers (`static:`, `+whole-archive`, `-bundle`)
//!
//! # How Static Detection Works
//!
//! For each `-l<name>` flag, the parser checks if `lib<name>.a` exists in any of the
//! `-L` directories. If the `.a` file exists and is **not** under a system root directory,
//! the library is linked statically. Otherwise, it's linked dynamically (letting the
//! system linker find the `.so`).
//!
//! This mirrors the logic from the [`pkg-config`](https://crates.io/crates/pkg-config) crate's
//! `is_static_available()` function.
//!
//! # Link Kinds
//!
//! Libraries are emitted with one of three link kinds:
//!
//! | Condition | Link Kind | Cargo Directive |
//! |-----------|-----------|----------------|
//! | No `.a` found (or in system dir) | `Default` | `rustc-link-lib=name` |
//! | `.a` exists, outside whole-archive region | `Static` | `rustc-link-lib=static:-bundle=name` |
//! | `.a` exists, inside whole-archive region | `WholeArchive` | `rustc-link-lib=static:+whole-archive,-bundle=name` |
//!
//! # Example
//!
//! ```no_run
//! use pkgconf::PkgConfigParser;
//!
//! // Basic usage with default settings
//! let parser = PkgConfigParser::new();
//!
//! parser.probe_and_emit(
//!     ["openssl", "libfoo"],
//!     None,
//! ).expect("pkg-config failed");
//! ```
//!
//! # Customization
//!
//! ```no_run
//! use pkgconf::PkgConfigParser;
//!
//! let parser = PkgConfigParser::new()
//!     // Add additional system roots (libs here link dynamically)
//!     .system_roots(["/usr", "/usr/local"])
//!     // Disable -bundle modifier (for non -sys crates)
//!     .no_bundle(false);
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

/// Represents how a library should be linked.
///
/// The link kind determines what cargo metadata directive is emitted:
/// - [`Default`](LinkKind::Default) → `cargo:rustc-link-lib=name`
/// - [`Static`](LinkKind::Static) → `cargo:rustc-link-lib=static:[-bundle]=name`
/// - [`WholeArchive`](LinkKind::WholeArchive) → `cargo:rustc-link-lib=static:+whole-archive[,-bundle]=name`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Let the linker decide (typically finds `.so` first, then `.a`).
    ///
    /// Used for system libraries like `pthread`, `numa`, `lz4` where the
    /// `.a` file either doesn't exist or is in a system directory.
    Default,

    /// Static library without `+whole-archive`.
    ///
    /// Used for libraries where a `.a` file exists in a non-system directory,
    /// but constructor functions don't need to be preserved.
    Static,

    /// Static library with `+whole-archive`.
    ///
    /// Required for libraries using constructor functions (e.g., DPDK's `RTE_INIT`
    /// macros) to ensure all symbols are included even if not directly referenced.
    /// Without this, the linker would discard "unused" constructor symbols.
    WholeArchive,
}

/// A parsed linker flag from pkg-config output.
///
/// These are the structured representations of flags parsed from
/// `pkg-config --static --libs` output.
#[derive(Debug, Clone)]
pub enum LinkerFlag {
    /// Library search path (`-L/path/to/libs`).
    ///
    /// Emitted as `cargo:rustc-link-search=native=/path/to/libs`.
    SearchPath(String),

    /// Library to link (`-lname` or `-l:libname.a`).
    ///
    /// The [`LinkKind`] determines the exact cargo directive format.
    Library {
        /// Library name without the `lib` prefix or `.a`/`.so` suffix.
        name: String,
        /// How this library should be linked.
        kind: LinkKind,
    },

    /// Raw linker argument (`-Wl,--export-dynamic`, etc.).
    ///
    /// Only certain linker arguments are preserved (e.g., `--export-dynamic`,
    /// `--as-needed`). The `--whole-archive` markers are consumed internally
    /// and converted to [`LinkKind::WholeArchive`] on affected libraries.
    LinkerArg(String),
}

/// Parser for pkg-config output that properly handles `--whole-archive` regions
/// and auto-detects static library availability.
///
/// # Usage
///
/// ```no_run
/// use pkgconf::PkgConfigParser;
///
/// PkgConfigParser::new()
///     .probe_and_emit(
///         ["openssl", "libfoo"],
///         None,
///     )
///     .expect("pkg-config failed");
/// ```
#[derive(Debug, Clone)]
pub struct PkgConfigParser {
    /// Whether to add `-bundle` modifier to static libraries.
    ///
    /// When `true` (default), emits `static:-bundle=` which prevents the
    /// static library from being re-exported when this crate is used as
    /// an rlib dependency. This is typically desired for `-sys` crates
    /// that use the `links` key in `Cargo.toml`.
    no_bundle: bool,

    /// Directories considered "system" roots.
    ///
    /// Libraries whose `.a` files are found under these directories will
    /// be linked with [`LinkKind::Default`] (dynamic linking), even if
    /// the `.a` file exists. Default: `["/usr"]`.
    system_roots: Vec<PathBuf>,

    /// Libraries that should always be linked with `+whole-archive`.
    ///
    /// This overrides the normal detection and forces these libraries
    /// to use [`LinkKind::WholeArchive`] even if not in a `--whole-archive`
    /// region from pkg-config. Useful for libraries with constructor
    /// functions (like SPDK event subsystem registration) where the
    /// pkg-config file doesn't include whole-archive flags.
    force_whole_archive: HashSet<String>,
}

impl Default for PkgConfigParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PkgConfigParser {
    /// Creates a new parser with default settings.
    ///
    /// Defaults:
    /// - `no_bundle`: `true` (adds `-bundle` modifier)
    /// - `system_roots`: `["/usr"]`
    /// - `force_whole_archive`: `[]` (empty)
    pub fn new() -> Self {
        Self {
            no_bundle: true,
            system_roots: vec![PathBuf::from("/usr")],
            force_whole_archive: HashSet::new(),
        }
    }

    /// Sets whether to add the `-bundle` modifier to static libraries.
    ///
    /// When `true` (default), static libraries are emitted with `-bundle`
    /// (e.g., `static:-bundle=foo`), which prevents them from being
    /// re-exported when this crate is used as an rlib dependency.
    ///
    /// Set to `false` if you want downstream crates to also link against
    /// these static libraries.
    pub fn no_bundle(mut self, enabled: bool) -> Self {
        self.no_bundle = enabled;
        self
    }

    /// Sets the system root directories.
    ///
    /// Libraries whose `.a` files are found under these directories will
    /// be linked dynamically (using [`LinkKind::Default`]), even if the
    /// static library exists. This is useful for system libraries like
    /// `lz4`, `numa`, or `openssl` that should use the system's shared
    /// library rather than a static copy.
    ///
    /// Default: `["/usr"]`
    ///
    /// # Example
    ///
    /// ```
    /// use pkgconf::PkgConfigParser;
    ///
    /// let parser = PkgConfigParser::new()
    ///     .system_roots(["/usr", "/usr/local", "/opt/homebrew"]);
    /// ```
    pub fn system_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.system_roots = roots.into_iter().map(|p| p.into()).collect();
        self
    }

    /// Sets libraries that should always use `+whole-archive`.
    ///
    /// These libraries will be linked with [`LinkKind::WholeArchive`] even if
    /// they don't appear inside a `--whole-archive` region in the pkg-config
    /// output. This is necessary for libraries that use constructor functions
    /// (like `__attribute__((constructor))` or DPDK's `RTE_INIT` macros)
    /// where the symbols would otherwise be discarded by the linker.
    ///
    /// # Example
    ///
    /// ```
    /// use pkgconf::PkgConfigParser;
    ///
    /// // Force whole-archive for libraries with constructor functions
    /// let parser = PkgConfigParser::new()
    ///     .force_whole_archive([
    ///         "mylib_with_constructors",
    ///     ]);
    /// ```
    pub fn force_whole_archive<I, S>(mut self, libs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.force_whole_archive = libs.into_iter().map(|s| s.as_ref().to_string()).collect();
        self
    }

    /// Runs `pkg-config --static --libs` and returns the raw output.
    ///
    /// # Arguments
    ///
    /// * `packages` - Package names to query (e.g., `["spdk_env_dpdk", "libdpdk"]`)
    /// * `pkg_config_path` - Optional path to set as `PKG_CONFIG_PATH` environment variable
    ///
    /// # Errors
    ///
    /// Returns an error if pkg-config is not found or if any package is not found.
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

    /// Checks if a static library (`.a`) is available in a non-system directory.
    ///
    /// Returns `true` if `lib<name>.a` exists in any of the provided directories
    /// and that directory is not under a system root. This is used to decide
    /// whether to force static linking or let the linker find a shared library.
    fn is_static_available(&self, name: &str, dirs: &[PathBuf]) -> bool {
        let libname = format!("lib{}.a", name);

        dirs.iter().any(|dir| {
            let library_exists = dir.join(&libname).exists();
            let is_system_dir = self.system_roots.iter().any(|sys| dir.starts_with(sys));
            library_exists && !is_system_dir
        })
    }

    /// Parse pkg-config output into structured linker flags.
    ///
    /// This function:
    /// - Tracks `--whole-archive` and `--no-whole-archive` markers
    /// - Checks if static libraries (.a) exist for each library
    /// - Libraries with .a in non-system dirs → Static or WholeArchive
    /// - Libraries without .a (or in system dirs) → Default (let linker find .so)
    /// - If a library appears first outside, then inside a whole-archive region,
    ///   it will be upgraded to WholeArchive.
    pub fn parse(&self, pkg_config_output: &str) -> Vec<LinkerFlag> {
        let mut flags = Vec::new();
        let mut seen_libs: HashSet<String> = HashSet::new();
        // Track library indices for upgrading to WholeArchive if seen again in whole-archive region
        let mut lib_indices: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        // Track whether we're inside a --whole-archive region from pkg-config
        let mut in_whole_archive_region = false;
        // Collect library search directories from -L flags
        let mut lib_dirs: Vec<PathBuf> = Vec::new();

        // First pass: collect all -L directories
        for flag in pkg_config_output.split_whitespace() {
            if let Some(path) = flag.strip_prefix("-L") {
                lib_dirs.push(PathBuf::from(path));
            }
        }

        // Second pass: parse all flags
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
                    &lib_dirs,
                );
            } else if let Some(lib_name) = flag.strip_prefix("-l") {
                self.handle_library(
                    &mut flags,
                    &mut seen_libs,
                    &mut lib_indices,
                    lib_name,
                    in_whole_archive_region,
                    &lib_dirs,
                );
            } else if flag == "-pthread" && !seen_libs.contains("pthread") {
                flags.push(LinkerFlag::Library {
                    name: "pthread".to_string(),
                    kind: LinkKind::Default,
                });
                seen_libs.insert("pthread".to_string());
            }
        }

        flags
    }

    /// Handles adding a library to the flags list, with deduplication and upgrade logic.
    ///
    /// If the library was already seen, checks if it needs to be upgraded from
    /// [`LinkKind::Static`] to [`LinkKind::WholeArchive`] (when it reappears inside
    /// a whole-archive region). Otherwise, adds the library with the appropriate
    /// link kind based on static availability and whole-archive context.
    fn handle_library(
        &self,
        flags: &mut Vec<LinkerFlag>,
        seen_libs: &mut HashSet<String>,
        lib_indices: &mut std::collections::HashMap<String, usize>,
        lib_name: &str,
        in_whole_archive_region: bool,
        lib_dirs: &[PathBuf],
    ) {
        if seen_libs.contains(lib_name) {
            // Library already seen - check if we need to upgrade to WholeArchive
            if in_whole_archive_region
                && let Some(&idx) = lib_indices.get(lib_name)
                && let LinkerFlag::Library { kind, .. } = &mut flags[idx]
                && *kind == LinkKind::Static
            {
                *kind = LinkKind::WholeArchive;
            }
            return;
        }

        // Determine link kind based on:
        // 1. Is it forced to be whole-archive?
        // 2. Is it in a whole-archive region?
        // 3. Does a static library (.a) exist in a non-system directory?
        let has_static = self.is_static_available(lib_name, lib_dirs);
        let forced_whole_archive = self.force_whole_archive.contains(lib_name);

        let kind = if (in_whole_archive_region || forced_whole_archive) && has_static {
            LinkKind::WholeArchive
        } else if has_static {
            LinkKind::Static
        } else {
            // No .a found or only in system dirs - let linker find .so
            LinkKind::Default
        };

        let idx = flags.len();
        flags.push(LinkerFlag::Library {
            name: lib_name.to_string(),
            kind,
        });
        seen_libs.insert(lib_name.to_string());
        lib_indices.insert(lib_name.to_string(), idx);
    }

    /// Emits cargo metadata directives for the parsed flags.
    ///
    /// Outputs `cargo:rustc-link-search`, `cargo:rustc-link-lib`, and
    /// `cargo:rustc-link-arg` directives to stdout for cargo to consume.
    ///
    /// Usually called via [`probe_and_emit`](Self::probe_and_emit), but can
    /// be called separately if you need to inspect or modify the parsed flags.
    pub fn emit_cargo_metadata(&self, flags: &[LinkerFlag]) {
        for flag in flags {
            match flag {
                LinkerFlag::SearchPath(path) => {
                    println!("cargo:rustc-link-search=native={}", path);
                }
                LinkerFlag::Library { name, kind } => match kind {
                    LinkKind::Default => {
                        // Let the linker decide static vs dynamic
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

    /// Runs pkg-config, parses the output, and emits cargo metadata.
    ///
    /// This is the main entry point for most use cases. It combines
    /// [`run_pkg_config`](Self::run_pkg_config), [`parse`](Self::parse),
    /// and [`emit_cargo_metadata`](Self::emit_cargo_metadata).
    ///
    /// # Arguments
    ///
    /// * `packages` - Package names to query
    /// * `pkg_config_path` - Optional `PKG_CONFIG_PATH` override
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pkgconf::PkgConfigParser;
    ///
    /// PkgConfigParser::new()
    ///     .probe_and_emit(
    ///         ["openssl", "libfoo"],
    ///         None,
    ///     )
    ///     .expect("pkg-config failed");
    /// ```
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

    /// Emits a cargo link directive for an additional library.
    ///
    /// Use this to add libraries that aren't in the pkg-config output
    /// but are still needed for linking.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pkgconf::{PkgConfigParser, LinkKind};
    ///
    /// let parser = PkgConfigParser::new();
    /// parser.add_extra_lib("custom_lib", LinkKind::Static);
    /// ```
    pub fn add_extra_lib(&self, name: &str, kind: LinkKind) {
        match kind {
            LinkKind::Default => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    fn create_test_dir_with_libs(libs: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for lib in libs {
            let path = dir.path().join(format!("lib{}.a", lib));
            File::create(&path).unwrap().write_all(b"").unwrap();
        }
        dir
    }

    #[test]
    fn test_is_static_available() {
        let dir = create_test_dir_with_libs(&["foo", "bar"]);
        let parser = PkgConfigParser::new();
        let dirs = vec![dir.path().to_path_buf()];

        assert!(parser.is_static_available("foo", &dirs));
        assert!(parser.is_static_available("bar", &dirs));
        assert!(!parser.is_static_available("baz", &dirs));
    }

    #[test]
    fn test_system_root_exclusion() {
        // Create a temp dir inside /tmp (not a system root)
        let dir = create_test_dir_with_libs(&["mylib"]);
        let parser = PkgConfigParser::new(); // default system_roots = ["/usr"]
        let dirs = vec![dir.path().to_path_buf()];

        // Should find it since /tmp is not under /usr
        assert!(parser.is_static_available("mylib", &dirs));

        // Now test with the dir as a system root
        let parser_with_root = PkgConfigParser::new().system_roots([dir.path()]);
        assert!(!parser_with_root.is_static_available("mylib", &dirs));
    }

    #[test]
    fn test_parse_with_static_detection() {
        let dir = create_test_dir_with_libs(&["spdk_env", "rte_mempool"]);
        let parser = PkgConfigParser::new();

        let output = format!("-L{} -lspdk_env -lpthread -lnuma", dir.path().display());
        let flags = parser.parse(&output);

        assert_eq!(flags.len(), 4);
        // spdk_env has .a → Static
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "spdk_env" && *kind == LinkKind::Static)
        );
        // pthread has no .a in test dir → Default
        assert!(
            matches!(&flags[2], LinkerFlag::Library { name, kind } if name == "pthread" && *kind == LinkKind::Default)
        );
        // numa has no .a in test dir → Default
        assert!(
            matches!(&flags[3], LinkerFlag::Library { name, kind } if name == "numa" && *kind == LinkKind::Default)
        );
    }

    #[test]
    fn test_whole_archive_region_with_static_detection() {
        let dir = create_test_dir_with_libs(&["spdk_log", "rte_mempool_ring", "rte_eal"]);
        let parser = PkgConfigParser::new();

        let output = format!(
            "-L{} -lspdk_log -Wl,--whole-archive -lrte_mempool_ring -lrte_eal -Wl,--no-whole-archive -lpthread",
            dir.path().display()
        );
        let flags = parser.parse(&output);

        assert_eq!(flags.len(), 5);
        // spdk_log before --whole-archive, has .a → Static
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "spdk_log" && *kind == LinkKind::Static)
        );
        // rte_mempool_ring inside --whole-archive, has .a → WholeArchive
        assert!(
            matches!(&flags[2], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::WholeArchive)
        );
        // rte_eal inside --whole-archive, has .a → WholeArchive
        assert!(
            matches!(&flags[3], LinkerFlag::Library { name, kind } if name == "rte_eal" && *kind == LinkKind::WholeArchive)
        );
        // pthread after --no-whole-archive, no .a → Default
        assert!(
            matches!(&flags[4], LinkerFlag::Library { name, kind } if name == "pthread" && *kind == LinkKind::Default)
        );
    }

    #[test]
    fn test_upgrade_to_whole_archive_on_duplicate() {
        let dir = create_test_dir_with_libs(&["rte_mempool_ring"]);
        let parser = PkgConfigParser::new();

        let output = format!(
            "-L{} -lrte_mempool_ring -Wl,--whole-archive -l:librte_mempool_ring.a -Wl,--no-whole-archive",
            dir.path().display()
        );
        let flags = parser.parse(&output);

        // Should only have 2 entries (SearchPath + one library)
        assert_eq!(flags.len(), 2);
        // Should be upgraded to WholeArchive
        assert!(
            matches!(&flags[1], LinkerFlag::Library { name, kind } if name == "rte_mempool_ring" && *kind == LinkKind::WholeArchive)
        );
    }

    #[test]
    fn test_dedup_libs() {
        let dir = create_test_dir_with_libs(&["spdk_log"]);
        let parser = PkgConfigParser::new();

        let output = format!(
            "-L{} -lspdk_log -l:libspdk_log.a -lspdk_log",
            dir.path().display()
        );
        let flags = parser.parse(&output);

        // Should only have 2 entries (SearchPath + one library)
        assert_eq!(flags.len(), 2);
    }
}
