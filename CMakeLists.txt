cmake_minimum_required(VERSION 3.18.4)
project(oscar VERSION 0.1.0 LANGUAGES CXX)

add_subdirectory(dependency/spdlog EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)
# there is an option to add Boost as submodule as well, but the cloning is too
# painful
find_package(Boost REQUIRED)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_BUILD_TYPE "RelWithDebInfo")
include_directories(./)
add_compile_options(-Wall -Wextra -pedantic)
enable_testing()
set(CMAKE_CXX_CLANG_TIDY clang-tidy 
    --use-color
    "-header-filter=-${CMAKE_SOURCE_DIR}/dependency"
)

# useful when need to create source-less target that requires at least one
# source file
# https://stackoverflow.com/a/34234515
set(OSCAR_DUMMY_SOURCE "${CMAKE_BINARY_DIR}/_dummy.cpp")
file(WRITE "${OSCAR_DUMMY_SOURCE}" "")

add_subdirectory(core)
add_subdirectory(common)
add_subdirectory(replication/unreplicated)
add_subdirectory(replication/vr)
add_subdirectory(test)
