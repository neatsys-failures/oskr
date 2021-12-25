add_library(oscar_core INTERFACE)
target_link_libraries(oscar_core INTERFACE
    spdlog::spdlog
    Bitsery::bitsery
    Boost::boost
)

add_library(oscar_pch STATIC ${OSCAR_DUMMY_SOURCE})
target_link_libraries(oscar_pch PRIVATE oscar_core)
# what to include into precompiled header? seems like a hard question
# maybe include some more thing from `common` (currently seems only can prepare
# one pch for any specific target)
target_precompile_headers(oscar_pch PRIVATE
    Foundation.hpp
)
