fn main() {
    println!("cargo:rerun-if-changed=schemas/exit.mol");
    println!("cargo:rerun-if-env-changed=GW_EXIT_PROFILE_HASH");
    molecule_codegen::Compiler::new()
        .output_dir_set_default()
        .input_schema_file("schemas/exit.mol")
        .generate_code(molecule_codegen::Language::Rust)
        .run()
        .expect("generate exit schema");
    let hash = std::env::var("GW_EXIT_PROFILE_HASH").unwrap_or_else(|_| "00".repeat(32));
    let hash = hash.strip_prefix("0x").unwrap_or(&hash);
    assert!(
        hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()),
        "GW_EXIT_PROFILE_HASH must be 32-byte hex"
    );
    let bytes: Vec<_> = (0..32)
        .map(|i| u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16).unwrap())
        .collect();
    std::fs::write(
        std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("profile_hash.rs"),
        format!("pub const PROFILE_HASH: [u8; 32] = {:?};", bytes),
    )
    .unwrap();
}
