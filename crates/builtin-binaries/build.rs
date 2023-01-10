//! Build script for crate `gw-builtin-binaries` to bundle the resources.
use anyhow::{bail, Result};
use ckb_fixed_hash::{h256, H256};
use includedir_codegen::Compression;
use sha2::{Digest, Sha256};
use std::fmt::Debug;
use std::path::Path;

const BINARIES_DIR: &str = "builtin";

fn verify_checksum<P: AsRef<Path> + Debug>(expected_checksum: H256, path: P) -> Result<()> {
    let content = std::fs::read(&path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let actual_checmsum = H256(hasher.finalize().into());
    if actual_checmsum != expected_checksum {
        bail!(
            "checksum mismatch path {:?}, actual: {} expected: {}",
            path,
            actual_checmsum,
            expected_checksum
        );
    }
    Ok(())
}

fn main() {
    let mut bundled = includedir_codegen::start("BUNDLED");

    for (path, checksum) in [
        (
            "godwoken-scripts/meta-contract-generator",
            h256!("0xb1544a9736a1f47c00b1509f9a0b16d001c3c1e3ed7eafecd6f669f96398ef8e"),
        ),
        (
            "godwoken-scripts/sudt-generator",
            h256!("0xf543d621e9e1655b0f4c9291114e1680dfe25a2364637a9e67f3a0bc5a7c8054"),
        ),
        (
            "godwoken-scripts/eth-addr-reg-generator",
            h256!("0x2aa7b75eb466b754cff54463702860f8477c266c2446cea98cd26b82bf67d5dd"),
        ),
        (
            "godwoken-polyjuice-v1.1.5-beta/generator",
            h256!("0x1829c695c91572840352794505714113025d0d55c8d318c2e013c4c2c971cf02"),
        ),
        (
            "godwoken-polyjuice-v1.2.0/generator",
            h256!("0x1e48c135dcb6eb97a96295bbe7e9b104df05d7e48a8a115b4cfebc36f4555634"),
        ),
        (
            "godwoken-polyjuice-v1.4.0/generator",
            h256!("0xa8a7868cc051af1e61e7cee267a42d2a957beac4788653e8a335d9a72f578a9b"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator",
            h256!("0x2a42be6177fe639b72f3b93e24fa1c6e037cf80678f62a809c85fc90662daf16"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator_log",
            h256!("0xaaed6b05b36af25235c8534f5c84274c52ed6f7d8adb19641d0e8eb57972ada1"),
        ),
        (
            "godwoken-polyjuice-v1.4.5/generator",
            h256!("0xee766d4a0e56a4c46f8917d52847a20bf9dcc3cd38f705faceec81dc39cdd53a"),
        ),
        (
            "godwoken-polyjuice-v1.5.0/generator",
            h256!("0xf89d4eb312fa4b8df9a3acc0a25f870201106825d9a609612c61aa8263cda1b8"),
        ),
        (
            "godwoken-polyjuice/generator",
            h256!("0x9589669de5cb1b7a1b97bd5679f9e480be264a34958e4aea8f15504cab19f61d"),
        ),
    ] {
        let path = Path::new(BINARIES_DIR).join(path);
        verify_checksum(checksum, &path)
            .unwrap_or_else(|err| panic!("error {:?} processing {:?}", err, &path));
        bundled
            .add_file(path, Compression::Gzip)
            .expect("add files to resource bundle");
    }

    bundled.build("bundled.rs").expect("build resource bundle");
}
