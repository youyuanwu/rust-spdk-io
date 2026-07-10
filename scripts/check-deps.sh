#!/usr/bin/env bash
#
# check-deps.sh - Verify that the build dependencies for rust-spdk-io are
# installed on this machine.
#
# This mirrors the toolchain expected by the CI workflow
# (.github/workflows/spdk.yml) and the SPDK build. It only checks that the
# tools/libraries are present; it does not install anything.
#
# Exit status:
#   0 - all required dependencies are present
#   1 - one or more required dependencies are missing
#
# Usage:
#   ./scripts/check-deps.sh

set -u

# Resolve the repository root so the script works from any directory.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." &>/dev/null && pwd)"

# Minimum CMake version required by the top-level CMakeLists.txt.
CMAKE_MIN_VERSION="3.28"

missing=0
warnings=0

red()   { printf '\033[31m%s\033[0m' "$1"; }
green() { printf '\033[32m%s\033[0m' "$1"; }
yellow(){ printf '\033[33m%s\033[0m' "$1"; }

ok()    { printf '  [%s] %s\n'  "$(green OK)"   "$1"; }
fail()  { printf '  [%s] %s\n'  "$(red FAIL)"   "$1"; missing=$((missing + 1)); }
warn()  { printf '  [%s] %s\n'  "$(yellow WARN)" "$1"; warnings=$((warnings + 1)); }

# Compare two dotted version strings. Returns 0 if $1 >= $2.
version_ge() {
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

# Check that a command exists on PATH. Required by default; pass "optional"
# as the third argument to only warn when missing.
check_cmd() {
    local cmd="$1" purpose="$2" mode="${3:-required}"
    if command -v "$cmd" >/dev/null 2>&1; then
        ok "$cmd ($purpose)"
        return 0
    fi
    if [ "$mode" = "optional" ]; then
        warn "$cmd not found ($purpose)"
    else
        fail "$cmd not found ($purpose)"
    fi
    return 1
}

echo "Checking rust-spdk-io build dependencies..."
echo

echo "Core build tools:"
check_cmd git   "clone SPDK source"
check_cmd gcc   "C compiler"
check_cmd g++   "C++ compiler"
check_cmd make  "build SPDK"
check_cmd patch "apply SPDK patches"
check_cmd nasm  "assemble ISA-L"
check_cmd meson "SPDK/DPDK build system"
check_cmd ninja "SPDK/DPDK build backend"

# pkg-config may be provided by either the pkg-config or pkgconf package.
if command -v pkg-config >/dev/null 2>&1; then
    ok "pkg-config (locate SPDK libraries)"
elif command -v pkgconf >/dev/null 2>&1; then
    ok "pkgconf (locate SPDK libraries)"
else
    fail "pkg-config/pkgconf not found (locate SPDK libraries)"
fi

echo
echo "CMake:"
if command -v cmake >/dev/null 2>&1; then
    cmake_version="$(cmake --version | head -n1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
    if version_ge "$cmake_version" "$CMAKE_MIN_VERSION"; then
        ok "cmake $cmake_version (>= $CMAKE_MIN_VERSION required)"
    else
        fail "cmake $cmake_version is too old (>= $CMAKE_MIN_VERSION required)"
    fi
else
    fail "cmake not found (>= $CMAKE_MIN_VERSION required)"
fi

echo
echo "Python:"
if check_cmd python3 "SPDK build scripts"; then
    # pyelftools (the 'elftools' module) is needed by DPDK's pmdinfogen.py to
    # parse driver ELF objects during the build. SPDK's pkgdep step also
    # installs it into a private venv (/var/spdk/dependencies/pip), so a
    # missing system module is only a warning, not a hard failure.
    spdk_pip_venv="/var/spdk/dependencies/pip"
    if python3 -c "import elftools" >/dev/null 2>&1; then
        ok "python3 pyelftools module (used by DPDK pmdinfogen)"
    elif [ -x "${spdk_pip_venv}/bin/python3" ] &&
        "${spdk_pip_venv}/bin/python3" -c "import elftools" >/dev/null 2>&1; then
        ok "pyelftools available via SPDK venv ${spdk_pip_venv}"
    else
        warn "python3 pyelftools module not found (install python3-pyelftools, or it is provided by 'cmake --build build --target spdk_pkgdep')"
    fi
fi

echo
echo "Rust:"
rust_channel=""
if [ -f "${REPO_ROOT}/rust-toolchain.toml" ]; then
    rust_channel="$(grep -oE 'channel[[:space:]]*=[[:space:]]*"[^"]+"' "${REPO_ROOT}/rust-toolchain.toml" | grep -oE '"[^"]+"' | tr -d '"')"
fi
if check_cmd cargo "build Rust crates" && command -v rustc >/dev/null 2>&1; then
    rustc_version="$(rustc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1)"
    if [ -n "$rust_channel" ] && [ "$rustc_version" != "$rust_channel" ]; then
        warn "rustc $rustc_version differs from rust-toolchain.toml channel $rust_channel (rustup will fetch $rust_channel on build)"
    else
        ok "rustc $rustc_version"
    fi
fi

echo
echo "SPDK (optional - built/installed via cmake targets):"
SPDK_PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-/opt/spdk/lib/pkgconfig}"
if command -v pkg-config >/dev/null 2>&1 &&
    PKG_CONFIG_PATH="$SPDK_PKG_CONFIG_PATH" pkg-config --exists spdk_env_dpdk 2>/dev/null; then
    spdk_version="$(PKG_CONFIG_PATH="$SPDK_PKG_CONFIG_PATH" pkg-config --modversion spdk_env_dpdk 2>/dev/null)"
    ok "SPDK ${spdk_version:-installed} (PKG_CONFIG_PATH=$SPDK_PKG_CONFIG_PATH)"
else
    warn "SPDK not found via pkg-config (build with 'cmake --build build --target spdk_install', then set PKG_CONFIG_PATH=$SPDK_PKG_CONFIG_PATH)"
fi

echo
if [ "$missing" -gt 0 ]; then
    printf '%s: %d required dependency(ies) missing, %d warning(s).\n' "$(red FAILED)" "$missing" "$warnings"
    echo "Install SPDK build deps with: cmake --build build --target spdk_pkgdep"
    exit 1
fi

printf '%s: all required dependencies present (%d warning(s)).\n' "$(green OK)" "$warnings"
exit 0
