add_library(oscar_common INTERFACE)
target_link_libraries(oscar_common INTERFACE oscar_core)

add_library(oscar_common_pch STATIC "${OSCAR_DUMMY_SOURCE}")
target_link_libraries(oscar_common_pch PUBLIC oscar_common)
target_precompile_headers(oscar_common_pch PUBLIC
    BasicClient.hpp
    ClientTable.hpp
)
