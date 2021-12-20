cmake_minimum_required(VERSION 3.18.4)
project(oscar VERSION 0.1.0 LANGUAGES CXX)

add_subdirectory(dependency/spdlog EXCLUDE_FROM_ALL)
add_subdirectory(dependency/boost EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_BUILD_TYPE "RelWithDebInfo")
include_directories(./)
# no-exceptions: not really about overhead, mostly hope program crash fast
# without unwinding, to ease debug with core dump
add_compile_options(-Wall -Wextra -pedantic -fno-exceptions)
enable_testing()

add_subdirectory(core)
add_subdirectory(common)

add_subdirectory(replication/unreplicated)

add_subdirectory(test)
