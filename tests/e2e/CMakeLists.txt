# E2E test target
add_custom_target(e2e-test
    COMMAND ${CMAKE_COMMAND} -E env ANSIBLE_NOCOLOR=1 ${CMAKE_CURRENT_SOURCE_DIR}/run-test.sh
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
    COMMENT "Running E2E tests"
)