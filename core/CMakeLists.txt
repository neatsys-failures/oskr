add_library(oscar_core INTERFACE)
target_link_libraries(oscar_core INTERFACE
    spdlog::spdlog
    Bitsery::bitsery
    Boost::boost
)
