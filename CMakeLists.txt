cmake_minimum_required(VERSION 3.18.4)
project(oscar VERSION 0.1.0 LANGUAGES CXX)

add_subdirectory(dependency/spdlog EXCLUDE_FROM_ALL)
add_subdirectory(dependency/boost EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)

enable_testing()
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_BUILD_TYPE "RelWithDebInfo")
include_directories(./)
add_compile_options(-Wall -Wextra -pedantic)

add_subdirectory(core)
add_subdirectory(replication/unreplicated)
add_subdirectory(test)
