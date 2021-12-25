add_executable(Client Client.cpp)
target_link_libraries(Client PRIVATE oscar_core PkgConfig::DPDK)