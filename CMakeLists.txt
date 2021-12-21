cmake_minimum_required(VERSION 3.18.4)
project(oscar VERSION 0.1.0 LANGUAGES CXX)

add_subdirectory(dependency/spdlog EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)
find_package(Boost)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_BUILD_TYPE "RelWithDebInfo")
include_directories(./)
# no-exceptions: not really about overhead, mostly hope program crash fast
# without unwinding, to ease debug with core dump
add_compile_options(-Wall -Wextra -pedantic)
enable_testing()

# useful when need to create source-less target that requires at least one
# source file
# https://stackoverflow.com/a/34234515
set(OSCAR_DUMMY_SOURCE "${CMAKE_BINARY_DIR}/_dummy.cpp")
file(WRITE "${OSCAR_DUMMY_SOURCE}" "")

add_subdirectory(core)
add_subdirectory(common)

add_library(oscar_pch STATIC "${OSCAR_DUMMY_SOURCE}")
target_link_libraries(oscar_pch PRIVATE oscar_core_pch oscar_common_pch)

add_subdirectory(replication/unreplicated)

add_subdirectory(test)
