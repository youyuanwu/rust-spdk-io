# pkgconf

Parses `pkg-config` output into structured types for both **linking** (`--libs`) and **bindgen** (`--cflags`), with proper `--whole-archive` and static library support.

## Why not the `pkg-config` crate?

1. It reorders `-Wl,--whole-archive` / `-l` flags, breaking libraries with constructor functions (DPDK's `RTE_INIT`, SPDK's `SPDK_SUBSYSTEM_REGISTER`)
2. It doesn't auto-detect static vs dynamic linking based on `.a` file availability

## Usage

```rust
use pkgconf::PkgConfigParser;

// In build.rs — single call for both linking and bindgen
let pkg = PkgConfigParser::new()
    .force_whole_archive(["mylib_with_constructors"])
    .probe(["spdk_env_dpdk", "libdpdk"], None)
    .expect("pkg-config failed");

// Emit cargo linker directives (no_bundle=true for -sys crates)
pkgconf::emit_cargo_metadata(&pkg.libs, true);

// Get clang args for bindgen
let clang_args = pkgconf::to_clang_args(&pkg.cflags);
bindgen::Builder::default()
    .header("wrapper.h")
    .clang_args(&clang_args)
    .generate()
    .expect("bindgen failed");
```

## Link Kinds

| Condition | Cargo Directive |
|-----------|-----------------|
| No `.a` found (or in system dir) | `rustc-link-lib=name` |
| `.a` exists, outside whole-archive region | `rustc-link-lib=static:-bundle=name` |
| `.a` exists, inside whole-archive region | `rustc-link-lib=static:+whole-archive,-bundle=name` |

## License

MIT
