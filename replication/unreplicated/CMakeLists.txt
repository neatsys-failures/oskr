add_library(oscar_replication_unreplicated INTERFACE)
target_precompile_headers(oscar_replication_unreplicated INTERFACE
    Replica.hpp
)
target_link_libraries(oscar_replication_unreplicated INTERFACE 
    oscar_core
    oscar_common
)
