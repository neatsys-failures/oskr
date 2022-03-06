use std::process::Command;
use std::str;

fn main() {
    // should we disable all DPDK stuff for test profile?

    let dpdk_cflags = Command::new("pkg-config")
        .args(["--cflags", "--static", "libdpdk-uninstalled"])
        .env("PKG_CONFIG_PATH", "target/dpdk/meson-uninstalled")
        .output()
        .unwrap()
        .stdout;
    let dpdk_cflags: Vec<_> = str::from_utf8(&dpdk_cflags)
        .unwrap()
        .split_whitespace()
        .collect();

    let dpdk_libs = Command::new("pkg-config")
        .args(["--libs", "libdpdk-uninstalled"])
        .env("PKG_CONFIG_PATH", "target/dpdk/meson-uninstalled")
        .output()
        .unwrap()
        .stdout;
    let dpdk_libs: Vec<_> = str::from_utf8(&dpdk_libs)
        .unwrap()
        .split_whitespace()
        .collect();

    let mut build = cc::Build::new();
    let mut build = build
        .file("src/dpdk_shim.c")
        // any better way?
        .flag("-march=native");
    for flag in dpdk_cflags {
        build = build.flag(flag);
    }
    build.compile("dpdk_shim");

    for flag in dpdk_libs {
        println!("cargo:rustc-link-arg={}", flag);
    }
    println!("cargo:rustc-link-arg=-lc");
    println!("cargo:rustc-link-arg=-Wl,-rpath=target/dpdk/lib");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
}
