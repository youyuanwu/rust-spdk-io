cmake_minimum_required(VERSION 3.16)
project(bypass-misc2 LANGUAGES)

# VM management targets
add_custom_target(vm-start
    COMMAND ${CMAKE_SOURCE_DIR}/tests/infra/start-vm.sh -y
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/tests/infra
    COMMENT "Starting test VM with KVM"
)

add_custom_target(vm-start-no-kvm
    COMMAND ${CMAKE_SOURCE_DIR}/tests/infra/start-vm.sh --no-kvm -y
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/tests/infra
    COMMENT "Starting test VM without KVM (software emulation)"
)

add_custom_target(vm-destroy
    COMMAND ${CMAKE_SOURCE_DIR}/tests/infra/destroy-vm.sh -y
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/tests/infra
    COMMENT "Destroying test VM"
)

add_custom_target(vm-destroy-no-kvm
    COMMAND ${CMAKE_SOURCE_DIR}/tests/infra/destroy-vm.sh --no-kvm -y
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/tests/infra
    COMMENT "Destroying test VM (no-kvm config)"
)

# E2E test target
add_custom_target(e2e-test
    COMMAND ${CMAKE_SOURCE_DIR}/tests/e2e/run-test.sh
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}/tests/e2e
    COMMENT "Running E2E tests"
)
