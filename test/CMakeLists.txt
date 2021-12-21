include(GoogleTest)

set(OSCAR_TEST_UNITS
    SimulatedTransport
    Misc
    Unreplicated
    BasicClient
)

foreach(UNIT ${OSCAR_TEST_UNITS})
    add_executable(Test${UNIT} ${UNIT}.cpp)
    target_link_libraries(Test${UNIT} PRIVATE 
        oscar_core 
        oscar_common 
        gtest_main
    )
    target_precompile_headers(Test${UNIT} REUSE_FROM oscar_pch)
    gtest_discover_tests(Test${UNIT})
endforeach()
