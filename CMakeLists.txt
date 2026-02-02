cmake_minimum_required(VERSION 3.28)
project(bypass-misc2 LANGUAGES)

add_subdirectory(tests)


include(FetchContent)

message(STATUS "Fetching SPDK source code...")
FetchContent_Populate(
  spdk
  GIT_REPOSITORY https://github.com/spdk/spdk.git
  GIT_TAG        2ef883ef96e79c3cc16da02f667a7a58c2453f2f # v26.01
  GIT_SHALLOW    TRUE
)

message(STATUS "SPDK source code fetched to: ${spdk_SOURCE_DIR}")
message(STATUS "SPDK build directory: ${spdk_BINARY_DIR}")

# Shallow update of submodules
message(STATUS "Updating SPDK submodules (shallow)...")
execute_process(
  COMMAND git submodule update --init --recursive --depth 1
  WORKING_DIRECTORY ${spdk_SOURCE_DIR}
  RESULT_VARIABLE GIT_SUBMODULE_RESULT
)

if(NOT GIT_SUBMODULE_RESULT EQUAL "0")
  message(FATAL_ERROR "git submodule update failed: ${GIT_SUBMODULE_RESULT}")
endif()

# Run manually:
add_custom_target(spdk_pkgdep
    COMMAND ./scripts/pkgdep.sh
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Installing SPDK package dependencies"
)

# Let SPDK build its bundled DPDK (system DPDK 22.11 is too old for SPDK v26.01)
add_custom_target(configure_spdk
    COMMAND ./configure --with-shared --without-rdma --without-fio --without-rbd --without-dpdk --without-ocf --without-crypto --disable-tests --disable-unit-tests
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Configuring SPDK"
)

add_custom_target(build_spdk
    COMMAND $(MAKE)
    WORKING_DIRECTORY ${spdk_SOURCE_DIR}
    COMMENT "Building SPDK"
)