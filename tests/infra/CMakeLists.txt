# VM management targets
add_custom_target(vm-start
    COMMAND ${CMAKE_CURRENT_SOURCE_DIR}/start-vm.sh -y --no-color
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Starting test VM with KVM"
)

add_custom_target(vm-start-no-kvm
    COMMAND ${CMAKE_CURRENT_SOURCE_DIR}/start-vm.sh --no-kvm -y --no-color
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Starting test VM without KVM (software emulation)"
)

add_custom_target(vm-destroy
    COMMAND ${CMAKE_CURRENT_SOURCE_DIR}/destroy-vm.sh -y --no-color
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Destroying test VM"
)

add_custom_target(vm-destroy-no-kvm
    COMMAND ${CMAKE_CURRENT_SOURCE_DIR}/destroy-vm.sh --no-kvm -y --no-color
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Destroying test VM (no-kvm config)"
)
