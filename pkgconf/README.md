# pkgconf

A Rust build helper for parsing pkg-config output with proper `--whole-archive` and static library support.

## Problem

The standard [`pkg-config`](https://crates.io/crates/pkg-config) crate has limitations:

1. **Flag reordering** — It doesn't preserve the ordering of `-Wl,--whole-archive` and `-l` flags, breaking linking for libraries with constructor functions (like DPDK's `RTE_INIT` macros)
2. **Static detection** — It doesn't properly distinguish between static and dynamic libraries based on `.a` file availability

## Solution

This crate parses pkg-config output directly and:

- **Tracks `--whole-archive` regions** from the pkg-config output
- **Auto-detects static library availability** by checking if `lib<name>.a` exists
- **Excludes system directories** (default: `/usr`) so system libs link dynamically
- **Emits correct cargo link directives** with proper modifiers (`static:`, `+whole-archive`, `-bundle`)

## Usage

```rust
use pkgconf::PkgConfigParser;

// In build.rs
fn main() {
    PkgConfigParser::new()
        .probe_and_emit(
            ["openssl", "libfoo"],
            None,  // or Some("/custom/pkgconfig/path")
        )
        .expect("pkg-config failed");
}
```

### Force Whole-Archive

For libraries with constructor functions that need `--whole-archive`:

```rust
use pkgconf::PkgConfigParser;

PkgConfigParser::new()
    .force_whole_archive([
        "mylib_with_constructors",
    ])
    .probe_and_emit(["mylib"], None)
    .expect("pkg-config failed");
```

### Custom System Roots

Libraries under system roots link dynamically (even if `.a` exists):

```rust
use pkgconf::PkgConfigParser;

PkgConfigParser::new()
    .system_roots(["/usr", "/usr/local", "/opt/homebrew"])
    .probe_and_emit(["openssl"], None)
    .expect("pkg-config failed");
```

## Link Kinds

Libraries are emitted with one of three link kinds:

| Condition | Link Kind | Cargo Directive |
|-----------|-----------|-----------------|
| No `.a` found (or in system dir) | `Default` | `rustc-link-lib=name` |
| `.a` exists, outside whole-archive region | `Static` | `rustc-link-lib=static:-bundle=name` |
| `.a` exists, inside whole-archive region | `WholeArchive` | `rustc-link-lib=static:+whole-archive,-bundle=name` |

## Why Constructor Functions Need Whole-Archive

Libraries using `__attribute__((constructor))` or macros like DPDK's `RTE_INIT` register functionality at load time. Without `--whole-archive`, the linker discards these "unused" symbols, causing registration to fail silently.

## License

MIT
