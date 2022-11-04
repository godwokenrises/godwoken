use std::env;
use std::path::Path;

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = Path::new(&dir).parent().unwrap().parent().unwrap();
    env::set_current_dir(root_dir).unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let mut build = cc::Build::new();

    // building
    build
        .file("c/rust-binding/smt_wrapper.c")
        .static_flag(true)
        .flag("-O3")
        .flag("-fno-builtin-printf")
        .flag("-fno-builtin-memcmp")
        .flag("-nostdinc")
        .flag("-nostdlib")
        .flag("-fvisibility=hidden")
        .flag("-Wl,-static")
        .flag("-fdata-sections")
        .flag("-ffunction-sections")
        .flag("-Wl,--gc-sections")
        // sparse-merkle-tree
        .include("c/deps/sparse-merkle-tree/c")
        // ckb-c-stdlib
        .include("c/deps/ckb-c-stdlib")
        .include("c/deps/ckb-c-stdlib/libc")
        .include("c/deps/molecule")
        .include("c/build")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-Wno-unused-parameter")
        .flag("-Wno-nonnull")
        .flag("-Wno-nonnull-compare")
        .flag("-Wno-unused-function")
        .define("__SHARED_LIBRARY__", None);

    if target_arch == "riscv64" {
        build.flag("-nostartfiles").compile("ckb-smt.a");
    } else {
        // skip build for non riscv64
        build.define("CKB_STDLIB_NO_SYSCALL_IMPL", None);
        build.compile("ckb-smt.a");
    }
}
