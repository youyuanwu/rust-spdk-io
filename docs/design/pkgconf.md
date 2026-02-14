# pkgconf: Unified pkg-config parsing for linking and bindgen

## Status

Implemented

## Problem

`spdk-io-sys/build.rs` depended on **two** pkg-config crates:

| Crate | Purpose |
|-------|---------|
| `pkgconf` (ours) | Linker flags with `--whole-archive` handling |
| `pkg-config` (third-party) | Include paths for bindgen |

The `pkg-config` crate was used in a loop over every SPDK library (~20 separate
invocations) solely to collect `-I` include paths for bindgen. This was wasteful
and created a redundant dependency.

## Solution

The `pkgconf` crate now handles both concerns with a single `probe()` entry point
that returns a [`PkgConfig`](../../pkgconf/src/lib.rs) struct containing parsed
linker flags (`--libs`) and compiler flags (`--cflags`).

### Architecture

```
PkgConfigParser::probe(packages)
    ├── pkg-config --static --libs  →  Vec<LinkerFlag>   →  emit_cargo_metadata()
    └── pkg-config --cflags         →  Vec<CompilerFlag>  →  to_clang_args()
```

**Parser** (`PkgConfigParser`) is purely about parsing. It is configured with
`system_roots` and `force_whole_archive`, and produces structured types.

**Conversion** is the caller's responsibility:
- `LinkerFlag::to_cargo_directive(no_bundle)` — the `-bundle` modifier is a
  cargo/rustc concern applied at emit time, not baked into parsed data
- `CompilerFlag::to_clang_arg()` — converts to `-I`/`-D` strings for bindgen

### Usage in `spdk-io-sys/build.rs`

```rust
let pkg = PkgConfigParser::new()
    .force_whole_archive(["spdk_event_bdev", "spdk_nvme", /* ... */])
    .probe(spdk_libs, Some(&pkg_config_path))
    .expect("pkg-config failed");

pkgconf::emit_cargo_metadata(&pkg.libs, true);

let clang_args = pkgconf::to_clang_args(&pkg.cflags);
bindgen::Builder::default()
    .header("wrapper.h")
    .clang_args(&clang_args)
    // ...
```

## Key types

| Type | Purpose |
|------|---------|
| `PkgConfig` | Result struct: `{ libs: Vec<LinkerFlag>, cflags: Vec<CompilerFlag> }` |
| `LinkerFlag` | `SearchPath`, `Library { name, kind }`, `LinkerArg` |
| `CompilerFlag` | `IncludePath(PathBuf)`, `Define { key, value }` |
| `LinkKind` | `Default`, `Static`, `WholeArchive` |

## Changes made

| File | Change |
|------|--------|
| [pkgconf/src/lib.rs](../../pkgconf/src/lib.rs) | Added `PkgConfig`, `CompilerFlag`, conversion methods, `probe()`, `parse_cflags()`. Removed `no_bundle` from parser. |
| [spdk-io-sys/build.rs](../../spdk-io-sys/build.rs) | Single `probe()` call replaces both crates. |
| [spdk-io-sys/Cargo.toml](../../spdk-io-sys/Cargo.toml) | Removed `pkg-config` build-dependency. |
| [Cargo.toml](../../Cargo.toml) | Removed `pkg-config` from workspace dependencies. |
