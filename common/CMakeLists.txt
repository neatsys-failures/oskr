add_library(oscar_common INTERFACE)
target_precompile_headers(oscar_common INTERFACE
    common/BasicClient.hpp
    common/ClientTable.hpp
)
target_link_libraries(oscar_common INTERFACE oscar_core)
