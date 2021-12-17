add_executable(TestSimulatedTransport SimulatedTransport.cpp)
target_link_libraries(TestSimulatedTransport PRIVATE gtest_main spdlog::spdlog)

include(GoogleTest)
gtest_discover_tests(
    TestSimulatedTransport
)
