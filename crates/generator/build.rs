use std::env;

fn main() {
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch == "x86_64" && (target_family == "windows" || target_family == "unix") {
        println!("cargo:rustc-cfg=has_aot");
    }
}
