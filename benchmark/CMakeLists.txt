add_executable(Client Client.cpp)
target_link_libraries(Client PRIVATE oskr_core PkgConfig::DPDK)