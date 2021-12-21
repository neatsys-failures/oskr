add_library(oscar_core INTERFACE)
target_link_libraries(oscar_core INTERFACE
    spdlog::spdlog
    Bitsery::bitsery
    Boost::boost
)

add_library(oscar_core_pch STATIC "${OSCAR_DUMMY_SOURCE}")
target_link_libraries(oscar_core_pch PUBLIC oscar_core)
target_precompile_headers(oscar_core_pch PUBLIC
    Client.hpp
    Config.hpp
    Foundation.hpp
    Log.hpp
    Transport.hpp
    TransportReceiver.hpp
    Type.hpp
)
