include(GoogleTest)

set(OSKR_TEST_UNITS
    Simulated
    Misc
    Unreplicated
    BasicClient
    VR
)

foreach(UNIT ${OSKR_TEST_UNITS})
    add_executable(Test${UNIT} ${UNIT}.cpp)
    target_link_libraries(Test${UNIT} PRIVATE 
        oskr_core 
        oskr_common 
        gtest_main
    )
    gtest_discover_tests(Test${UNIT})
endforeach()

target_link_libraries(TestUnreplicated PRIVATE oskr_replication_unreplicated)
target_link_libraries(TestVR PRIVATE oskr_replication_vr)
