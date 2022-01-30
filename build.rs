use pkg_config::probe_library;
use std::env;
use std::ops::Deref;

fn main() {
    env::set_var("PKG_CONFIG_PATH", "target/dpdk/meson-uninstalled");
    let dpdk = probe_library("libdpdk-uninstalled").unwrap();

    let mut build = cc::Build::new();
    let mut build = build
        .file("src/dpdk_shim.c")
        .includes(dpdk.include_paths)
        // any better way?
        .flag("-march=native");
    for (var, val) in &dpdk.defines {
        build = build.define(var, val.as_ref().map(Deref::deref));
    }
    build.compile("dpdk_shim");

    for lib in dpdk.libs {
        println!("cargo:rustc-link-lib={}", lib);
    }
    for link_path in dpdk.link_paths {
        println!("cargo:rustc-link-search={}", link_path.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath={}", link_path.display());
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
}
