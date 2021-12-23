add_library(oscar_replication_unreplicated INTERFACE)
target_link_libraries(oscar_replication_unreplicated INTERFACE 
    oscar_core
    oscar_common
)
