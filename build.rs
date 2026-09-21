fn main() {
    println!("cargo:rerun-if-env-changed=COMPOSITOR_UPDATE_URL");
    let root = "Compositor/Rendering";
    let mut build = cc::Build::new();
    build.include(root).flag_if_supported("-std=gnu11");
    for name in [
        "HealPixels",
        "ContentFill",
        "LensPixels",
        "NoisePixels",
        "WandPixels",
    ] {
        let source = format!("{root}/{name}.c");
        build.file(&source);
        println!("cargo:rerun-if-changed={source}");
        println!("cargo:rerun-if-changed={root}/{name}.h");
    }
    build.compile("compositor_pixels");
    println!("cargo:rustc-link-lib=m");
}
