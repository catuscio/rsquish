use std::{env, path::PathBuf};

fn prefix(name: &str, fallback: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed={name}");
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback))
}

fn archive(directory: &std::path::Path, library: &str) {
    let path = directory.join(format!("lib{library}.a"));
    assert!(
        path.is_file(),
        "Missing static archive {}. Run bash native/build-libavif.sh and install the documented static development dependencies.",
        path.display()
    );
    println!("cargo:rerun-if-changed={}", path.display());
    // Apple ld can prefer a same-name dylib for direct test/binary links. Keep
    // uniquely named archives in OUT_DIR and expose no Homebrew library path.
    let isolated = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let destination = isolated.join(format!("librk_{library}.a"));
    if destination.exists() {
        std::fs::remove_file(&destination).expect("replace isolated archive");
    }
    std::fs::copy(&path, &destination).expect("copy static archive into isolated linker directory");
    println!("cargo:rustc-link-search=native={}", isolated.display());
    println!("cargo:rustc-link-lib=static=rk_{library}");
}

fn main() {
    let project = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let avif_build = prefix(
        "RASTER_AVIF_STATIC_DIR",
        project.join("native/libavif-build").to_str().unwrap(),
    );
    let aom = prefix("RASTER_AOM_PREFIX", "/opt/homebrew/opt/aom");
    let vmaf = prefix("RASTER_VMAF_PREFIX", "/opt/homebrew/opt/libvmaf");
    let jpeg = prefix("RASTER_MOZJPEG_PREFIX", "/opt/homebrew/opt/mozjpeg");
    let webp = prefix("RASTER_WEBP_PREFIX", "/opt/homebrew/opt/webp");
    println!("cargo:rerun-if-changed=native/codecs.c");
    println!("cargo:rerun-if-changed=../../native/libavif/include/avif/avif.h");
    cc::Build::new()
        .file("native/codecs.c")
        .include(project.join("native/libavif/include"))
        .include(jpeg.join("include"))
        .include(webp.join("include"))
        .flag_if_supported("-std=c11")
        .warnings(true)
        .compile("rasterkit_codecs");
    archive(&jpeg.join("lib"), "jpeg");
    archive(&webp.join("lib"), "webp");
    archive(&webp.join("lib"), "sharpyuv");
    archive(&avif_build, "avif");
    archive(&aom.join("lib"), "aom");
    archive(&vmaf.join("lib"), "vmaf");
    // libaom contains C++ objects; only the platform C++ runtime remains dynamic.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=c++");
    } else if env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=m");
        println!("cargo:rustc-link-lib=pthread");
    }
}
