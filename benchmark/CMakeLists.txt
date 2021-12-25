add_executable(Client Client.cpp)
target_link_libraries(Client PRIVATE oscar_core PkgConfig::DPDK)
target_precompile_headers(Client REUSE_FROM oscar_pch)
