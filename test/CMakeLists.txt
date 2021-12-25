include(GoogleTest)

set(OSCAR_TEST_UNITS
    Simulated
    Misc
    Unreplicated
    BasicClient
    VR
)

foreach(UNIT ${OSCAR_TEST_UNITS})
    add_executable(Test${UNIT} ${UNIT}.cpp)
    target_precompile_headers(Test${UNIT} REUSE_FROM oscar_pch)
    target_link_libraries(Test${UNIT} PRIVATE 
        oscar_core 
        oscar_common 
        gtest_main
    )
    gtest_discover_tests(Test${UNIT})
endforeach()

target_link_libraries(TestUnreplicated PRIVATE oscar_replication_unreplicated)
target_link_libraries(TestVR PRIVATE oscar_replication_vr)
