include(GoogleTest)

set(OSKR_TEST_UNITS
    Simulated
    Misc
    Unreplicated
    BasicClient
    VR
)
set(OSKR_TEST_TARGET run-test)

foreach(UNIT ${OSKR_TEST_UNITS})
    add_executable(Test${UNIT} ${UNIT}.cpp)
    target_link_libraries(Test${UNIT} PRIVATE 
        oskr_core 
        oskr_common 
        gtest_main)
    list(APPEND OSKR_TEST_TARGET COMMAND Test${UNIT})
    add_custom_target(run-Test${UNIT} 
        COMMAND Test${UNIT} 
        WORK_DIRECTORY ${CMAKE_BINARY_DIRECTORY})
endforeach()
add_custom_target(${OSKR_TEST_TARGET} WORK_DIRECTORY ${CMAKE_BINARY_DIRECTORY})

target_link_libraries(TestUnreplicated PRIVATE oskr_replication_unreplicated)
target_link_libraries(TestVR PRIVATE oskr_replication_vr)
