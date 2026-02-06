cmake_minimum_required(VERSION 3.28)
project(bypass-misc2 LANGUAGES)

add_subdirectory(tests)


include(FetchContent)

# SPDK v26.01 uses DPDK 25.11.0 (spdk-25.11 fork branch)
# DPDK commit: e01bfcd05fe39f628392c8b29b880f5692de2224
message(STATUS "Fetching SPDK source code...")
FetchContent_Declare(
  spdk
  GIT_REPOSITORY https://github.com/spdk/spdk.git
  GIT_TAG        2ef883ef96e79c3cc16da02f667a7a58c2453f2f # v26.01
  GIT_SHALLOW    TRUE
  GIT_SUBMODULES libvfio-user dpdk isa-l isa-l-crypto
  GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(spdk)

message(STATUS "SPDK source code fetched to: ${spdk_SOURCE_DIR}")
message(STATUS "SPDK build directory: ${spdk_BINARY_DIR}")

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
# Note: SPDK does not support true out-of-tree builds - must build in source directory
add_custom_target(configure_spdk
    COMMAND ${CMAKE_COMMAND} -E env ${SPDK_CC_ENV} ./configure 
      --prefix=/opt/spdk
      --with-shared --without-rdma --without-fio --without-rbd 
      --without-dpdk --without-ocf --without-crypto 
      --disable-tests --disable-unit-tests --target-arch=nehalem
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

# Include SPDK .deb packaging target
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/spdk_deb.cmake)

# Add custom target to install SPDK to system via .deb package
add_custom_target(spdk_install
    COMMAND apt install -y ${CMAKE_CURRENT_BINARY_DIR}/spdk_${SPDK_VERSION}_amd64.deb
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
    COMMENT "Installing SPDK .deb package"
    # DEPENDS spdk_deb
)

# Add custom target to uninstall SPDK package
add_custom_target(spdk_uninstall
    COMMAND apt remove -y spdk
    COMMENT "Uninstalling SPDK package"
)