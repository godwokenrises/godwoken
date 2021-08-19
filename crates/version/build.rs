fn main() {
    // get commit id
    let commit_id = std::process::Command::new("git")
        .args(&[
            "describe",
            "--always",
            "--match",
            "__EXCLUDE__",
            "--abbrev=7",
        ])
        .output()
        .ok()
        .and_then(|r| String::from_utf8(r.stdout).ok())
        .expect("commit id");
    println!("cargo:rustc-env=COMMIT_ID={}", commit_id);
}
