cmake_minimum_required(VERSION 3.18.4)
project(oskr VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_BUILD_TYPE Debug)
if (${OSKR_BENCHMARK})
    set(CMAKE_BUILD_TYPE Release)
    add_compile_definitions(OSKR_BENCHMARK)
endif()


# happy easy way to set up DPDK
if (NOT EXISTS "${CMAKE_BINARY_DIR}/dependency/dpdk/build.ninja")
    execute_process(
        COMMAND bash -c "meson setup ${CMAKE_BINARY_DIR}/dependency/dpdk"
        WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}/dependency/dpdk")
endif()
execute_process(
    COMMAND ninja
    WORKING_DIRECTORY "${CMAKE_BINARY_DIR}/dependency/dpdk") 
find_package(PkgConfig REQUIRED)
set(ENV{PKG_CONFIG_PATH} "${CMAKE_BINARY_DIR}/dependency/dpdk/meson-uninstalled")
pkg_check_modules(DPDK REQUIRED IMPORTED_TARGET libdpdk)
file(MAKE_DIRECTORY ${DPDK_INCLUDE_DIRS}) # https://stackoverflow.com/a/47358004

add_subdirectory(dependency/bitsery EXCLUDE_FROM_ALL)
add_subdirectory(dependency/folly EXCLUDE_FROM_ALL)
add_subdirectory(dependency/googletest EXCLUDE_FROM_ALL)
set(use_libclang ON)
set(use_libc++ OFF)
add_subdirectory(dependency/doxygen EXCLUDE_FROM_ALL)


set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
include_directories(./)
add_compile_options(-Wall -Wextra -Wpedantic)
add_compile_options(-Wno-gnu-zero-variadic-macro-arguments) # for logging
add_compile_options(-Wno-nullability-completeness) # for folly
enable_testing()
if (${OSKR_TIDY})  # use a indirect command line option to prevent affect dependencies
    set(CMAKE_CXX_CLANG_TIDY clang-tidy --use-color)
endif()
# https://stackoverflow.com/a/34234515
set(OSKR_DUMMY_SOURCE "${CMAKE_BINARY_DIR}/_dummy.cpp")
file(WRITE "${OSKR_DUMMY_SOURCE}" "")


add_subdirectory(core)
add_subdirectory(common)
add_subdirectory(replication/unreplicated)
add_subdirectory(replication/vr)
add_subdirectory(benchmark)
add_subdirectory(test)

# document setup
add_custom_target(doc
    COMMAND doxygen
    COMMAND sphinx-build -M html doc build/doc
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
)
# generate a proper compile_commands.json for Doxygen's Clang assistant parsing
# to play with
# proper == only one item with all dependencies' flags
# doc target not need to depend on this, since compile_commands.json will be 
# generated anyway
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
add_library(oskr_doxygen STATIC ${OSKR_DUMMY_SOURCE})
# TODO link to local targets
target_link_libraries(oskr_doxygen PRIVATE
    Bitsery::bitsery
    folly
    PkgConfig::DPDK
)

# prevent side effects
unset(OSKR_BENCHMARK CACHE)
unset(OSKR_TIDY CACHE)