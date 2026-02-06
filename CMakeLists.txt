cmake_minimum_required(VERSION 3.28)
project(bypass-misc2 LANGUAGES)

add_subdirectory(tests)


include(FetchContent)

# SPDK v26.01 uses DPDK 25.11.0 (spdk-25.11 fork branch)
# DPDK commit: e01bfcd05fe39f628392c8b29b880f5692de2224
message(STATUS "Fetching SPDK source code...")
FetchContent_Populate(
  spdk
  GIT_REPOSITORY https://github.com/spdk/spdk.git
  GIT_TAG        2ef883ef96e79c3cc16da02f667a7a58c2453f2f # v26.01
  GIT_SHALLOW    TRUE
  GIT_SUBMODULES ""  # Don't auto-init submodules, we'll do it manually
)

message(STATUS "SPDK source code fetched to: ${spdk_SOURCE_DIR}")
message(STATUS "SPDK build directory: ${spdk_BINARY_DIR}")

# Initialize only required submodules (skip dpdk - using system DPDK)
# Skip: dpdk (system), intel-ipsec-mb (crypto disabled), isa-l-crypto (crypto disabled), ocf (disabled), xnvme (not needed)
message(STATUS "Updating SPDK submodules (only isa-l and libvfio-user)...")
execute_process(
  COMMAND git submodule update --init --depth 1 -- isa-l libvfio-user
  WORKING_DIRECTORY ${spdk_SOURCE_DIR}
  RESULT_VARIABLE GIT_SUBMODULE_RESULT
)

if(NOT GIT_SUBMODULE_RESULT EQUAL "0")
  message(FATAL_ERROR "git submodule update failed: ${GIT_SUBMODULE_RESULT}")
endif()

# Apply SPDK patches (idempotent - checks if already applied)
set(SPDK_PATCH_FILE ${CMAKE_CURRENT_SOURCE_DIR}/patches/spdk-v26.01.patch)
set(SPDK_PATCH_MARKER ${spdk_SOURCE_DIR}/.spdk-patched)

if(EXISTS ${SPDK_PATCH_FILE} AND NOT EXISTS ${SPDK_PATCH_MARKER})
  message(STATUS "Applying SPDK patches...")
  # Remove version.py if it exists (patch creates it, but it might exist from a previous partial apply)
  file(REMOVE ${spdk_SOURCE_DIR}/python/spdk/version.py)
  execute_process(
    COMMAND patch -p1 --forward -i ${SPDK_PATCH_FILE}
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    RESULT_VARIABLE PATCH_RESULT
  )
  if(PATCH_RESULT EQUAL "0")
    file(WRITE ${SPDK_PATCH_MARKER} "Patched with ${SPDK_PATCH_FILE}\n")
    message(STATUS "SPDK patches applied successfully")
  else()
    message(WARNING "Patch may have already been applied or failed: ${PATCH_RESULT}")
  endif()
elseif(EXISTS ${SPDK_PATCH_MARKER})
  message(STATUS "SPDK patches already applied, skipping")
endif()

# Download DPDK deb package for VM installation
set(DPDK_DEB_URL "https://github.com/youyuanwu/rust-dpdk-net/releases/download/v0.1.1/dpdk-net_25.11.0_amd64.deb")
set(DPDK_DEB_FILE "${CMAKE_CURRENT_BINARY_DIR}/dpdk-net_25.11.0_amd64.deb")
set(DPDK_DEB_SHA256 "68459b98a0eb5224016d4362ece04fb9383c16fc5ee207777bc99fe9e78c88d6")
if(NOT EXISTS ${DPDK_DEB_FILE})
  message(STATUS "Downloading DPDK deb package...")
  file(DOWNLOAD ${DPDK_DEB_URL} ${DPDK_DEB_FILE}
    SHOW_PROGRESS
    STATUS DPDK_DOWNLOAD_STATUS
    EXPECTED_HASH SHA256=${DPDK_DEB_SHA256}
  )
  list(GET DPDK_DOWNLOAD_STATUS 0 DPDK_DOWNLOAD_RESULT)
  if(NOT DPDK_DOWNLOAD_RESULT EQUAL 0)
    message(WARNING "Failed to download DPDK deb: ${DPDK_DOWNLOAD_STATUS}")
  else()
    message(STATUS "DPDK deb downloaded to: ${DPDK_DEB_FILE}")
  endif()
else()
  message(STATUS "DPDK deb already exists: ${DPDK_DEB_FILE}")
endif()

# Run manually:
add_custom_target(spdk_pkgdep
    COMMAND ./scripts/pkgdep.sh
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Installing SPDK package dependencies"
)

# Detect compiler and sccache
# Check if CMAKE_C_COMPILER_LAUNCHER is set to sccache
find_program(SCCACHE_PROGRAM sccache)
if(SCCACHE_PROGRAM)
  message(STATUS "sccache found: ${SCCACHE_PROGRAM}")
  # Check what compiler sccache would wrap
  if(CMAKE_C_COMPILER)
    set(SPDK_CC "${CMAKE_C_COMPILER}")
    set(SPDK_CXX "${CMAKE_CXX_COMPILER}")
  else()
    # Use system default 'cc' (matches SPDK's default behavior)
    find_program(SPDK_CC NAMES cc gcc clang)
    find_program(SPDK_CXX NAMES c++ g++ clang++)
  endif()
  # Determine compiler type for logging
  execute_process(
    COMMAND ${SPDK_CC} --version
    OUTPUT_VARIABLE CC_VERSION_OUTPUT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET
  )
  if(CC_VERSION_OUTPUT MATCHES "clang")
    set(SPDK_COMPILER_TYPE "clang")
  else()
    set(SPDK_COMPILER_TYPE "gcc")
  endif()
  message(STATUS "SPDK will use sccache with ${SPDK_COMPILER_TYPE}: CC=${SPDK_CC}, CXX=${SPDK_CXX}")
  # sccache wraps the compiler: CC="sccache /path/to/cc"
  set(SPDK_CC_ENV "CC=${SCCACHE_PROGRAM} ${SPDK_CC}" "CXX=${SCCACHE_PROGRAM} ${SPDK_CXX}")
else()
  message(STATUS "sccache not found, using default compiler")
  set(SPDK_CC_ENV "")
endif()

# Use system DPDK 25.11.0 (detected via pkg-config)
# Use --target-arch=nehalem for broad compatibility (SSE4.2). QEMU TCG can emulate this.
# ISA-L is only used by crypto module (disabled via --without-crypto), so no libisal dependency
add_custom_target(configure_spdk
    COMMAND ${CMAKE_COMMAND} -E env ${SPDK_CC_ENV} ./configure --with-shared --without-rdma --without-fio --without-rbd --with-dpdk --without-ocf --without-crypto --disable-tests --disable-unit-tests --target-arch=nehalem
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Configuring SPDK"
)

add_custom_target(build_spdk
    COMMAND $(MAKE)
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Building SPDK"
)

add_custom_target(clean_spdk
    COMMAND $(MAKE) clean
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Cleaning SPDK build"
)

# Add custom target to install SPDK to system
add_custom_target(spdk_install
    COMMAND $(MAKE) install
    COMMAND ldconfig
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Installing SPDK artifacts"
    DEPENDS build_spdk
)

# Add custom target to create .deb package without installing to system
# Uses DESTDIR staging with dpkg-deb - no extra tools required
set(SPDK_VERSION "26.01")
set(SPDK_PKG_DIR ${CMAKE_CURRENT_BINARY_DIR}/spdk-pkg)

# Create the DEBIAN control file content
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/spdk-control.txt
"Package: spdk
Version: ${SPDK_VERSION}
Section: libs
Priority: optional
Architecture: amd64
Depends: libc6, libnuma1, libssl3t64 | libssl3, libaio1t64 | libaio1, libfuse3-3, libuuid1, libiscsi7
Maintainer: maintainer@example.com
Description: Storage Performance Development Kit (SPDK) - custom build
")

add_custom_target(spdk_deb
    # Clean previous package directory
    COMMAND ${CMAKE_COMMAND} -E remove_directory ${SPDK_PKG_DIR}
    COMMAND ${CMAKE_COMMAND} -E make_directory ${SPDK_PKG_DIR}/DEBIAN
    # Install SPDK to staging directory (skip Python which doesn't respect DESTDIR)
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/lib install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/module install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/shared_lib install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/include install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/app install
    # Install bundled ISA-L library if it was built (depends on nasm being present during configure)
    COMMAND ${CMAKE_COMMAND} -DSRC=${spdk_SOURCE_DIR}/isa-l/.libs/libisal.so.2 -DDST=${SPDK_PKG_DIR}/usr/local/lib -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/copy_if_exists.cmake
    # Copy control file
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_BINARY_DIR}/spdk-control.txt ${SPDK_PKG_DIR}/DEBIAN/control
    # Build the .deb package
    COMMAND dpkg-deb --build ${SPDK_PKG_DIR} ${CMAKE_CURRENT_BINARY_DIR}/spdk_${SPDK_VERSION}_amd64.deb
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
    COMMENT "Creating SPDK .deb package (staged, no system install)"
    DEPENDS build_spdk
)