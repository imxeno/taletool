use std::path::Path;

fn main() {
    let vendor = Path::new("vendor").join("zlib-1.1.2");
    let sources = ["adler32.c", "deflate.c", "trees.c", "zutil.c"];

    println!("cargo:rerun-if-changed=src/zlib112_shim.c");
    for source in &sources {
        println!("cargo:rerun-if-changed={}", vendor.join(source).display());
    }
    println!("cargo:rerun-if-changed={}", vendor.join("zlib.h").display());
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("zconf.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        vendor.join("zutil.h").display()
    );

    let mut build = cc::Build::new();
    build
        .include(&vendor)
        .define("Z_PREFIX", None)
        .warnings(false)
        .file("src/zlib112_shim.c");
    for source in &sources {
        build.file(vendor.join(source));
    }
    build.compile("taletool_zlib112");
}
