fn main() {
    let mut build = cc::Build::new();

    // building
    build
        .file("../c/rust-binding/uint256_wrapper.c")
        .static_flag(true)
        .flag("-O3")
        .flag("-Wl,-static")
        .flag("-Wl,--gc-sections")
        .include("../c/")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-nonnull")
        .flag("-Wno-nonnull-compare")
        .flag("-Wno-unused-function")
        .define("__SHARED_LIBRARY__", None)
        .compile("c-uint256.a");
}
