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
//! - **Produces structured types** ([`LinkerFlag`], [`CompilerFlag`]) that can be converted
//!   to cargo metadata directives or clang arguments for bindgen
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
//! let pkg = PkgConfigParser::new()
//!     .probe(["openssl", "libfoo"], None)
//!     .expect("pkg-config failed");
//!
//! // Emit cargo linker directives (no_bundle=true for -sys crates)
//! pkgconf::emit_cargo_metadata(&pkg.libs, true);
//!
//! // Get clang args for bindgen
//! let clang_args = pkgconf::to_clang_args(&pkg.cflags);
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
//!     // Force whole-archive for libs with constructor functions
//!     .force_whole_archive(["mylib_with_constructors"]);
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

impl LinkerFlag {
    /// Converts this flag to a cargo metadata directive string.
    ///
    /// `no_bundle` controls whether static libraries get the `-bundle` modifier.
    /// This is a cargo/rustc concern (prevents re-exporting static libs through rlibs)
    /// and is applied here at emit time, not during parsing.
    ///
    /// # Examples
    ///
    /// ```
    /// use pkgconf::{LinkerFlag, LinkKind};
    ///
    /// let flag = LinkerFlag::SearchPath("/opt/spdk/lib".to_string());
    /// assert_eq!(flag.to_cargo_directive(true), "cargo:rustc-link-search=native=/opt/spdk/lib");
    ///
    /// let flag = LinkerFlag::Library { name: "foo".to_string(), kind: LinkKind::Static };
    /// assert_eq!(flag.to_cargo_directive(true), "cargo:rustc-link-lib=static:-bundle=foo");
    /// assert_eq!(flag.to_cargo_directive(false), "cargo:rustc-link-lib=static=foo");
    ///
    /// let flag = LinkerFlag::Library { name: "bar".to_string(), kind: LinkKind::WholeArchive };
    /// assert_eq!(flag.to_cargo_directive(true), "cargo:rustc-link-lib=static:+whole-archive,-bundle=bar");
    /// assert_eq!(flag.to_cargo_directive(false), "cargo:rustc-link-lib=static:+whole-archive=bar");
    /// ```
    pub fn to_cargo_directive(&self, no_bundle: bool) -> String {
        match self {
            LinkerFlag::SearchPath(path) => {
                format!("cargo:rustc-link-search=native={}", path)
            }
            LinkerFlag::Library { name, kind } => match kind {
                LinkKind::Default => {
                    format!("cargo:rustc-link-lib={}", name)
                }
                LinkKind::Static => {
                    if no_bundle {
                        format!("cargo:rustc-link-lib=static:-bundle={}", name)
                    } else {
                        format!("cargo:rustc-link-lib=static={}", name)
                    }
                }
                LinkKind::WholeArchive => {
                    if no_bundle {
                        format!(
                            "cargo:rustc-link-lib=static:+whole-archive,-bundle={}",
                            name
                        )
                    } else {
                        format!("cargo:rustc-link-lib=static:+whole-archive={}", name)
                    }
                }
            },
            LinkerFlag::LinkerArg(arg) => {
                format!("cargo:rustc-link-arg={}", arg)
            }
        }
    }
}

/// A parsed compiler flag from `pkg-config --cflags` output.
///
/// These flags are **not** consumed by cargo or rustc — they are used as
/// clang arguments for bindgen when generating FFI bindings from C headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilerFlag {
    /// Include search path (`-I/path/to/headers`).
    ///
    /// Passed to bindgen as `-I/path/to/headers` so clang can find
    /// `#include <spdk/env.h>` etc.
    IncludePath(PathBuf),

    /// Preprocessor define (`-DFOO` or `-DFOO=bar`).
    ///
    /// Passed to bindgen as `-DFOO` or `-DFOO=bar`. Some libraries
    /// (e.g., DPDK) emit defines like `-DRTE_MACHINE_CPUFLAG_SSE` that
    /// affect which code paths are visible in headers.
    Define {
        /// The macro name.
        key: String,
        /// The macro value, if any.
        value: Option<String>,
    },
}

impl CompilerFlag {
    /// Converts this flag to a clang argument string for bindgen.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use pkgconf::CompilerFlag;
    ///
    /// let flag = CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include"));
    /// assert_eq!(flag.to_clang_arg(), "-I/opt/spdk/include");
    ///
    /// let flag = CompilerFlag::Define { key: "FOO".to_string(), value: None };
    /// assert_eq!(flag.to_clang_arg(), "-DFOO");
    ///
    /// let flag = CompilerFlag::Define { key: "FOO".to_string(), value: Some("1".to_string()) };
    /// assert_eq!(flag.to_clang_arg(), "-DFOO=1");
    /// ```
    pub fn to_clang_arg(&self) -> String {
        match self {
            CompilerFlag::IncludePath(path) => format!("-I{}", path.display()),
            CompilerFlag::Define { key, value: None } => format!("-D{}", key),
            CompilerFlag::Define {
                key,
                value: Some(v),
            } => format!("-D{}={}", key, v),
        }
    }
}

/// Converts a slice of [`CompilerFlag`]s to clang argument strings for bindgen.
///
/// # Example
///
/// ```
/// use std::path::PathBuf;
/// use pkgconf::{CompilerFlag, to_clang_args};
///
/// let flags = vec![
///     CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include")),
///     CompilerFlag::Define { key: "FOO".to_string(), value: None },
/// ];
/// let args = to_clang_args(&flags);
/// assert_eq!(args, vec!["-I/opt/spdk/include", "-DFOO"]);
/// ```
pub fn to_clang_args(flags: &[CompilerFlag]) -> Vec<String> {
    flags.iter().map(|f| f.to_clang_arg()).collect()
}

/// Converts a slice of [`LinkerFlag`]s to cargo metadata directive strings.
///
/// `no_bundle` controls whether static libraries get the `-bundle` modifier.
/// Set to `true` for `-sys` crates that use the `links` key in `Cargo.toml`.
pub fn to_cargo_directives(flags: &[LinkerFlag], no_bundle: bool) -> Vec<String> {
    flags
        .iter()
        .map(|f| f.to_cargo_directive(no_bundle))
        .collect()
}

/// Emits cargo metadata directives to stdout.
///
/// Convenience function that prints each directive from [`to_cargo_directives`].
///
/// `no_bundle` controls whether static libraries get the `-bundle` modifier.
/// Set to `true` for `-sys` crates that use the `links` key in `Cargo.toml`.
pub fn emit_cargo_metadata(flags: &[LinkerFlag], no_bundle: bool) {
    for directive in to_cargo_directives(flags, no_bundle) {
        println!("{directive}");
    }
}

/// Parsed pkg-config output for a set of packages.
///
/// Contains structured linker flags (from `--libs`) and compiler flags
/// (from `--cflags`). This is a pure data type — it holds what pkg-config
/// reported, with no cargo/rustc/bindgen concerns baked in.
///
/// Use [`to_clang_args`] to convert `cflags` for bindgen, and
/// [`emit_cargo_metadata`] or [`to_cargo_directives`] to convert `libs`
/// for cargo.
#[derive(Debug, Clone)]
pub struct PkgConfig {
    /// Linker flags from `pkg-config --static --libs`.
    pub libs: Vec<LinkerFlag>,
    /// Compiler flags from `pkg-config --cflags`.
    pub cflags: Vec<CompilerFlag>,
}

/// Parser for pkg-config output that properly handles `--whole-archive` regions
/// and auto-detects static library availability.
///
/// # Usage
///
/// ```no_run
/// use pkgconf::PkgConfigParser;
///
/// let pkg = PkgConfigParser::new()
///     .probe(["openssl", "libfoo"], None)
///     .expect("pkg-config failed");
///
/// // Emit cargo linker directives (no_bundle=true for -sys crates)
/// pkgconf::emit_cargo_metadata(&pkg.libs, true);
///
/// // Get clang args for bindgen
/// let clang_args = pkgconf::to_clang_args(&pkg.cflags);
/// ```
#[derive(Debug, Clone)]
pub struct PkgConfigParser {
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
    /// - `system_roots`: `["/usr"]`
    /// - `force_whole_archive`: `[]` (empty)
    pub fn new() -> Self {
        Self {
            system_roots: vec![PathBuf::from("/usr")],
            force_whole_archive: HashSet::new(),
        }
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

    /// Runs `pkg-config` with the given arguments and returns the raw output.
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments to pass before the package names (e.g., `["--static", "--libs"]`)
    /// * `packages` - Package names to query (e.g., `["spdk_env_dpdk", "libdpdk"]`)
    /// * `pkg_config_path` - Optional path to set as `PKG_CONFIG_PATH` environment variable
    ///
    /// # Errors
    ///
    /// Returns an error if pkg-config is not found or if any package is not found.
    fn run_pkg_config_raw<I, S>(
        args: &[&str],
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

        cmd.args(args);
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
        Self::run_pkg_config_raw(&["--static", "--libs"], packages, pkg_config_path)
    }

    /// Runs `pkg-config --cflags` and returns the raw output.
    ///
    /// # Arguments
    ///
    /// * `packages` - Package names to query (e.g., `["spdk_env_dpdk", "libdpdk"]`)
    /// * `pkg_config_path` - Optional path to set as `PKG_CONFIG_PATH` environment variable
    ///
    /// # Errors
    ///
    /// Returns an error if pkg-config is not found or if any package is not found.
    pub fn run_pkg_config_cflags<I, S>(
        packages: I,
        pkg_config_path: Option<&str>,
    ) -> Result<String, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::run_pkg_config_raw(&["--cflags"], packages, pkg_config_path)
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

    /// Parses `pkg-config --cflags` output into structured compiler flags.
    ///
    /// Handles:
    /// - `-I/path` → [`CompilerFlag::IncludePath`]
    /// - `-DFOO` → [`CompilerFlag::Define`] `{ key: "FOO", value: None }`
    /// - `-DFOO=bar` → [`CompilerFlag::Define`] `{ key: "FOO", value: Some("bar") }`
    ///
    /// Deduplicates flags (preserving first occurrence order).
    /// Unknown flags are silently ignored.
    pub fn parse_cflags(&self, output: &str) -> Vec<CompilerFlag> {
        let mut flags = Vec::new();
        let mut seen = HashSet::new();

        for token in output.split_whitespace() {
            if let Some(path) = token.strip_prefix("-I") {
                if seen.insert(token.to_string()) {
                    flags.push(CompilerFlag::IncludePath(PathBuf::from(path)));
                }
            } else if let Some(define) = token.strip_prefix("-D")
                && seen.insert(token.to_string())
            {
                if let Some((key, val)) = define.split_once('=') {
                    flags.push(CompilerFlag::Define {
                        key: key.to_string(),
                        value: Some(val.to_string()),
                    });
                } else {
                    flags.push(CompilerFlag::Define {
                        key: define.to_string(),
                        value: None,
                    });
                }
            }
            // Unknown flags (e.g., -std=c11) are silently ignored
        }

        flags
    }

    /// Runs pkg-config and parses both linker and compiler flags.
    ///
    /// Executes `pkg-config --static --libs` and `pkg-config --cflags`
    /// and returns the combined parsed result as a [`PkgConfig`].
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
    /// let pkg = PkgConfigParser::new()
    ///     .probe(["spdk_env_dpdk", "libdpdk"], None)
    ///     .expect("pkg-config failed");
    ///
    /// // Emit cargo linker directives (no_bundle=true for -sys crates)
    /// pkgconf::emit_cargo_metadata(&pkg.libs, true);
    ///
    /// // Get clang args for bindgen
    /// let clang_args = pkgconf::to_clang_args(&pkg.cflags);
    /// ```
    pub fn probe<I, S>(
        &self,
        packages: I,
        pkg_config_path: Option<&str>,
    ) -> Result<PkgConfig, String>
    where
        I: IntoIterator<Item = S> + Clone,
        S: AsRef<str>,
    {
        let libs_output = Self::run_pkg_config(packages.clone(), pkg_config_path)?;
        let cflags_output = Self::run_pkg_config_cflags(packages, pkg_config_path)?;

        Ok(PkgConfig {
            libs: self.parse(&libs_output),
            cflags: self.parse_cflags(&cflags_output),
        })
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

    #[test]
    fn test_parse_cflags_include_paths() {
        let parser = PkgConfigParser::new();
        let output = "-I/opt/spdk/include -I/usr/include/dpdk";
        let flags = parser.parse_cflags(output);

        assert_eq!(flags.len(), 2);
        assert_eq!(
            flags[0],
            CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include"))
        );
        assert_eq!(
            flags[1],
            CompilerFlag::IncludePath(PathBuf::from("/usr/include/dpdk"))
        );
    }

    #[test]
    fn test_parse_cflags_defines() {
        let parser = PkgConfigParser::new();
        let output = "-DFOO -DBAR=1 -DBAZ=hello";
        let flags = parser.parse_cflags(output);

        assert_eq!(flags.len(), 3);
        assert_eq!(
            flags[0],
            CompilerFlag::Define {
                key: "FOO".to_string(),
                value: None
            }
        );
        assert_eq!(
            flags[1],
            CompilerFlag::Define {
                key: "BAR".to_string(),
                value: Some("1".to_string())
            }
        );
        assert_eq!(
            flags[2],
            CompilerFlag::Define {
                key: "BAZ".to_string(),
                value: Some("hello".to_string())
            }
        );
    }

    #[test]
    fn test_parse_cflags_mixed_with_unknown() {
        let parser = PkgConfigParser::new();
        let output = "-I/opt/spdk/include -std=c11 -DFOO -Wall -I/usr/include/dpdk";
        let flags = parser.parse_cflags(output);

        // Unknown flags (-std=c11, -Wall) are silently ignored
        assert_eq!(flags.len(), 3);
        assert_eq!(
            flags[0],
            CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include"))
        );
        assert_eq!(
            flags[1],
            CompilerFlag::Define {
                key: "FOO".to_string(),
                value: None
            }
        );
        assert_eq!(
            flags[2],
            CompilerFlag::IncludePath(PathBuf::from("/usr/include/dpdk"))
        );
    }

    #[test]
    fn test_parse_cflags_dedup() {
        let parser = PkgConfigParser::new();
        let output = "-I/opt/spdk/include -I/opt/spdk/include -DFOO -DFOO";
        let flags = parser.parse_cflags(output);

        assert_eq!(flags.len(), 2);
    }

    #[test]
    fn test_to_clang_arg() {
        assert_eq!(
            CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include")).to_clang_arg(),
            "-I/opt/spdk/include"
        );
        assert_eq!(
            CompilerFlag::Define {
                key: "FOO".to_string(),
                value: None
            }
            .to_clang_arg(),
            "-DFOO"
        );
        assert_eq!(
            CompilerFlag::Define {
                key: "FOO".to_string(),
                value: Some("1".to_string())
            }
            .to_clang_arg(),
            "-DFOO=1"
        );
    }

    #[test]
    fn test_to_clang_args() {
        let flags = vec![
            CompilerFlag::IncludePath(PathBuf::from("/opt/spdk/include")),
            CompilerFlag::Define {
                key: "FOO".to_string(),
                value: None,
            },
        ];
        let args = to_clang_args(&flags);
        assert_eq!(args, vec!["-I/opt/spdk/include", "-DFOO"]);
    }

    #[test]
    fn test_to_cargo_directive_search_path() {
        let flag = LinkerFlag::SearchPath("/opt/spdk/lib".to_string());
        assert_eq!(
            flag.to_cargo_directive(true),
            "cargo:rustc-link-search=native=/opt/spdk/lib"
        );
        assert_eq!(
            flag.to_cargo_directive(false),
            "cargo:rustc-link-search=native=/opt/spdk/lib"
        );
    }

    #[test]
    fn test_to_cargo_directive_default_lib() {
        let flag = LinkerFlag::Library {
            name: "pthread".to_string(),
            kind: LinkKind::Default,
        };
        assert_eq!(
            flag.to_cargo_directive(true),
            "cargo:rustc-link-lib=pthread"
        );
        assert_eq!(
            flag.to_cargo_directive(false),
            "cargo:rustc-link-lib=pthread"
        );
    }

    #[test]
    fn test_to_cargo_directive_static_lib() {
        let flag = LinkerFlag::Library {
            name: "spdk_log".to_string(),
            kind: LinkKind::Static,
        };
        assert_eq!(
            flag.to_cargo_directive(true),
            "cargo:rustc-link-lib=static:-bundle=spdk_log"
        );
        assert_eq!(
            flag.to_cargo_directive(false),
            "cargo:rustc-link-lib=static=spdk_log"
        );
    }

    #[test]
    fn test_to_cargo_directive_whole_archive() {
        let flag = LinkerFlag::Library {
            name: "rte_eal".to_string(),
            kind: LinkKind::WholeArchive,
        };
        assert_eq!(
            flag.to_cargo_directive(true),
            "cargo:rustc-link-lib=static:+whole-archive,-bundle=rte_eal"
        );
        assert_eq!(
            flag.to_cargo_directive(false),
            "cargo:rustc-link-lib=static:+whole-archive=rte_eal"
        );
    }

    #[test]
    fn test_to_cargo_directive_linker_arg() {
        let flag = LinkerFlag::LinkerArg("-Wl,--export-dynamic".to_string());
        assert_eq!(
            flag.to_cargo_directive(true),
            "cargo:rustc-link-arg=-Wl,--export-dynamic"
        );
    }
}
