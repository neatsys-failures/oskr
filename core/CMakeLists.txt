add_library(oscar_core INTERFACE)
target_precompile_headers(oscar_core INTERFACE
    Client.hpp
    Config.hpp
    Foundation.hpp
    Transport.hpp
    TransportReceiver.hpp
    Type.hpp
)
target_link_libraries(oscar_core INTERFACE
    Boost::core
    spdlog::spdlog
)
