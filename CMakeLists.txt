cmake_minimum_required(VERSION 3.18.4)
project(oskr VERSION 0.1.0 LANGUAGES CXX)

if ("${CMAKE_BUILD_TYPE}" STREQUAL "")
    set(CMAKE_BUILD_TYPE Debug)
endif()

# happy easy way to set up DPDK
execute_process(
    COMMAND bash -c "test -f build/build.ninja || meson setup build"
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}/dependency/dpdk")
execute_process(
    COMMAND ninja -C build
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}/dependency/dpdk") 
find_package(PkgConfig REQUIRED)
set(ENV{PKG_CONFIG_PATH} "${CMAKE_SOURCE_DIR}/dependency/dpdk/build/meson-uninstalled")
pkg_check_modules(DPDK REQUIRED IMPORTED_TARGET libdpdk)
file(MAKE_DIRECTORY ${DPDK_INCLUDE_DIRS}) # https://stackoverflow.com/a/47358004

add_subdirectory(dependency/spdlog EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)
# there is an option to add Boost as submodule as well, but the cloning is too
# painful
find_package(Boost REQUIRED)


set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
include_directories(./)
add_compile_options(-Wall -Wextra -Wpedantic)
add_compile_options(-Wno-gnu-zero-variadic-macro-arguments) # for r-logging
enable_testing()
if (${OSKR_TIDY})
    set(CMAKE_CXX_CLANG_TIDY clang-tidy --use-color)
endif()
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# https://stackoverflow.com/a/34234515
set(OSKR_DUMMY_SOURCE "${CMAKE_BINARY_DIR}/_dummy.cpp")
file(WRITE "${OSKR_DUMMY_SOURCE}" "")


add_subdirectory(core)
add_subdirectory(common)
add_subdirectory(replication/unreplicated)
add_subdirectory(replication/vr)
add_subdirectory(test)
add_subdirectory(benchmark)

# prevent side effects
unset(CMAKE_BUILD_TYPE CACHE)
unset(OSKR_TIDY CACHE)