add_executable(TestSimulatedTransport SimulatedTransport.cpp)
target_link_libraries(TestSimulatedTransport gtest_main)

include(GoogleTest)
gtest_discover_tests(
    TestSimulatedTransport
)
