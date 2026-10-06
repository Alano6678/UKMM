use std::{env, path::Path};

use rustc_version::{version_meta, Channel};

#[cfg(feature = "yaz0")]
fn build_zlib() -> std::path::PathBuf {
    let target = env::var("TARGET").unwrap();
    let build_dir = std::path::PathBuf::from(env::var("OUT_DIR").unwrap()).join("zlib");
    let source_dir = std::path::PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("lib/zlib-ng");
    let profile = if env::var("DEBUG").unwrap_or_default() == "true" { "Debug" } else { "Release" };
    let mut cmake = std::process::Command::new("cmake");
    cmake.arg("-S").arg(&source_dir).arg("-B").arg(&build_dir)
        .arg("-DCMAKE_POLICY_VERSION_MINIMUM=3.5")
        .arg("-DZLIB_COMPAT=OFF").arg("-DWITH_GTEST=OFF")
        .arg("-DBUILD_SHARED_LIBS=OFF")
        .arg("-DZLIB_ENABLE_TESTS=OFF")
        .arg(format!("-DCMAKE_BUILD_TYPE={profile}"));
    if target.contains("aarch64-apple-darwin") {
        cmake
            .arg("-DCMAKE_OSX_ARCHITECTURES=arm64")
            .arg("-DWITH_NEON=OFF");
    } else if target.contains("x86_64-apple-darwin") {
        cmake.arg("-DCMAKE_OSX_ARCHITECTURES=x86_64");
    } else {
        //Not OSX
    }
    if target.contains("windows") {
        cmake.arg("-A").arg(if target.starts_with("aarch64") {"ARM64"} else {"x64"});
    }
    let configured = cmake.output().expect("CMake is required for zlib");
    assert!(configured.status.success(), "zlib configure failed: {}", String::from_utf8_lossy(&configured.stderr));
    let built = std::process::Command::new("cmake")
        .arg("--build")
        .arg(&build_dir).arg("--config").arg(profile).arg("--parallel").arg("4")
        .output()
        .expect("Failed to build zlib");
    assert!(built.status.success(), "zlib build failed: {}\n{}", String::from_utf8_lossy(&built.stdout), String::from_utf8_lossy(&built.stderr));
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-search=native={}", build_dir.join(profile).display());
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=static={}", if profile == "Debug" {"zlibd"} else {"zlib"});
    } else {
        println!("cargo:rustc-link-lib=static=zlib");
    }
    build_dir
}

#[cfg(feature = "yaz0")]
fn build_yaz0() {
    let zlib_dir = build_zlib();
    let mut builder = cxx_build::bridge("src/yaz0.rs");
    builder
        .file("src/yaz0.cpp")
        .flag("-w")
        .flag_if_supported("-std=c++17")
        .include("src/include")
        .include("lib/nonstd")
        .include(&zlib_dir)
        .include("lib/zlib-ng")
        .flag_if_supported("-static");
    if cfg!(windows) {
        builder
            .flag_if_supported("/std:c++17")
            .flag_if_supported("/W4")
            .flag_if_supported("/wd4244")
            .flag_if_supported("/wd4127")
            .flag_if_supported("/Zc:__cplusplus");
    } else {
        builder
            .flag_if_supported("-fcolor-diagnostics")
            .flag_if_supported("-Wall")
            .flag_if_supported("-Wextra")
            .flag_if_supported("-fno-plt");
    }
    builder.compile("roead");
    println!("cargo:rerun-if-changed=src/include/oead");
    println!("cargo:rerun-if-changed=src/yaz0.rs");
    println!("cargo:rerun-if-changed=src/yaz0.cpp");
    println!("cargo:rerun-if-changed=src/include/oead/yaz0.h");
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    println!(
        "cargo:rustc-link-search=native={}",
        Path::new(&dir).join("lib/zlib-ng").display()
    );
}

fn main() {
    // Set cfg flags depending on release channel
    let channel = match version_meta().unwrap().channel {
        Channel::Stable => "CHANNEL_STABLE",
        Channel::Beta => "CHANNEL_BETA",
        Channel::Nightly => "CHANNEL_NIGHTLY",
        Channel::Dev => "CHANNEL_DEV",
    };
    println!("cargo:rustc-cfg={}", channel);
    println!("cargo::rustc-check-cfg=cfg({})", channel);
    #[cfg(feature = "yaz0")]
    build_yaz0();
}
