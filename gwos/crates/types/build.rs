use std::fs::read_dir;

use molecule_codegen::{Compiler, Language};

fn main() {
    println!("cargo:rerun-if-changed=schemas");
    for f in read_dir("schemas").unwrap() {
        let f = f.unwrap();
        println!("generating code for {}", f.file_name().to_string_lossy());
        Compiler::new()
            .output_dir_set_default()
            .input_schema_file(f.path())
            .generate_code(Language::Rust)
            .run()
            .unwrap();
    }
}
