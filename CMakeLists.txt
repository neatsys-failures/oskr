cmake_minimum_required(VERSION 3.18.4)
project(oscar VERSION 0.1.0 LANGUAGES CXX)

enable_testing()
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_BUILD_TYPE "RelWithDebInfo")
include_directories(
    ./
    dep/
)

add_subdirectory(dep/googletest)

add_subdirectory(test)
