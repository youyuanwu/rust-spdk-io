# DPDK Mempool Linking Issue - RESOLVED

## Problem Summary

SPDK thread initialization was failing with:
```
spdk_msg_mempool creation failed
```
Return code: `-ENOMEM` (-12)

**Status: SOLVED** ✓

## Root Cause Analysis

### Call Chain
1. `spdk_thread_lib_init()` → `_thread_lib_init()` (thread.c:383)
2. `spdk_mempool_create("spdk_msg_mempool", ...)` (env.c:216)
3. `rte_mempool_create()` → `rte_mempool_set_ops_byname("ring_mp_mc")` (rte_mempool.c:943)
4. **FAILS**: `ring_mp_mc` ops not found in `rte_mempool_ops_table`

### Why Ops Are Missing

DPDK uses constructor functions to register mempool handlers at startup:

```c
// rte_mempool_ring.c (line 202-207)
static const struct rte_mempool_ops ops_mp_mc = {
    .name = "ring_mp_mc",
    .alloc = common_ring_alloc,
    ...
};

RTE_MEMPOOL_REGISTER_OPS(ops_mp_mc);  // Expands to RTE_INIT constructor
```

The `RTE_MEMPOOL_REGISTER_OPS` macro expands to:
```c
RTE_INIT(mp_hdlr_init_ops_mp_mc)
{
    rte_mempool_register_ops(&ops_mp_mc);
}
```

This creates an entry in `.init_array` section - a constructor called at program startup.

### The Linking Problem

When statically linking `librte_mempool_ring.a`:

1. **No direct references** exist to `mp_hdlr_init_ops_mp_mc()` - it's only called via `.init_array`
2. The linker performs dead code elimination and discards the entire object file
3. The `.init_array` entries are lost
4. At runtime, `rte_mempool_ops_table` is empty
5. `rte_mempool_set_ops_byname("ring_mp_mc")` fails → mempool creation fails

### Solution Required

Must use `--whole-archive` when linking `librte_mempool_ring.a` (and other DPDK libs with constructors) to force inclusion of all object files regardless of symbol references.

## Current Status

**RESOLVED** - All tests passing.

### Solution

The fix uses Cargo's link modifier syntax `static:+whole-archive,-bundle=libname` which:
1. Forces inclusion of all object files (preserving `.init_array` constructors)
2. Prevents re-bundling into the rlib (since spdk-io-sys uses `links = "spdk"`)

Key insight: The standard `pkg-config` crate reorders linker flags, breaking `--whole-archive` semantics. 
We bypass this by parsing pkg-config output directly and emitting `rustc-link-lib` with the `+whole-archive` modifier.

### Implementation

Created `spdk-io-build` crate with `PkgConfigParser` helper:

```rust
use spdk_io_build::{PkgConfigParser, COMMON_SYSTEM_LIBS, LinkKind};

let parser = PkgConfigParser::new()
    .system_libs(COMMON_SYSTEM_LIBS);  // pthread, numa, etc. → dynamic

parser.probe_and_emit(&spdk_libs, Some(&pkg_config_path))
    .expect("pkg-config failed");

// Emits: cargo:rustc-link-lib=static:+whole-archive,-bundle=rte_mempool_ring
//        cargo:rustc-link-lib=pthread  (dynamic for system libs)
```

### Attempted Solutions (Historical)

1. **`cargo:rustc-link-arg=-Wl,--whole-archive`** before libs - didn't work, Cargo reorders flags
2. **`static:+whole-archive=libname`** without `-bundle` - error "overriding linking modifiers not supported"  
3. **`--push-state,--whole-archive` with `-l` args** - partially worked but flag ordering still broken
4. **`static:+whole-archive,-bundle=libname`** ✓ - **WORKS** - this is the solution

## Verification Commands

Verify ring ops symbols are in binary:
```bash
nm target/debug/deps/thread_test-* | grep -E "ops_mp_mc|mp_hdlr_init"
# Should show: ops_mp_mc, mp_hdlr_init_ops_mp_mc
```

Run the mempool test:
```bash
cargo test -p spdk-io --test mempool_test -- test_mempool_create
# Should pass with "Mempool created successfully!"
```

## References

- DPDK RTE_INIT: Uses `.init_array` section for constructor registration
- Cargo link modifiers: `static:+whole-archive,-bundle=` (requires `-bundle` for `links` key crates)
- SPDK pkg-config: `/opt/spdk/lib/pkgconfig/`