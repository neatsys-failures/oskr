add_library(oskr_core INTERFACE)
target_link_libraries(oskr_core INTERFACE
    Bitsery::bitsery
    folly
)