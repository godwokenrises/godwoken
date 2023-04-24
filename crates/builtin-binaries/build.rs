#![cfg_attr(feature = "no-builtin", allow(dead_code), allow(unused_imports))]
//! Build script for crate `gw-builtin-binaries` to bundle the resources.
use anyhow::{bail, Result};
use ckb_fixed_hash::{h256, H256};
use includedir_codegen::Compression;
use sha2::{Digest, Sha256};
use std::fmt::Debug;
use std::path::Path;

const BINARIES_DIR: &str = "builtin";
const GWOS_EVM_DIR: &str = "../../gwos-evm/build";

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

#[cfg(feature = "no-builtin")]
fn main() {
    let bundled = includedir_codegen::start("BUNDLED");
    bundled.build("bundled.rs").expect("build resource bundle");
}

#[cfg(not(feature = "no-builtin"))]
fn main() {
    // Copy polyjuice v1.5.5 files
    {
        let target_path = Path::new(BINARIES_DIR).join("godwoken-polyjuice-v1.5.5/generator");
        if !target_path.exists() {
            let dir = target_path.parent().unwrap();
            std::fs::create_dir_all(dir).unwrap();

            let src = Path::new(GWOS_EVM_DIR).join("generator");
            std::fs::copy(src, dir.join("generator")).unwrap();

            let src = Path::new(GWOS_EVM_DIR).join("generator.debug");
            std::fs::copy(src, dir.join("generator.debug")).unwrap();
        }
    }

    let mut bundled = includedir_codegen::start("BUNDLED");

    for (path, checksum) in [
        (
            "gwos-v1.3.0-rc1/meta-contract-generator",
            h256!("0xb1544a9736a1f47c00b1509f9a0b16d001c3c1e3ed7eafecd6f669f96398ef8e"),
        ),
        (
            "gwos-v1.3.0-rc1/meta-contract-generator.debug",
            h256!("0x454156f031b4b59981e82d6e6f7acf74f5070bba83a1fcecd80493b0a900a99c"),
        ),
        (
            "gwos-v1.3.0-rc1/sudt-generator",
            h256!("0xf543d621e9e1655b0f4c9291114e1680dfe25a2364637a9e67f3a0bc5a7c8054"),
        ),
        (
            "gwos-v1.3.0-rc1/sudt-generator.debug",
            h256!("0x926bdc1bbddfb40a243a8841b258aa3f0838934fc1e29d3ec539c785d795ecff"),
        ),
        (
            "gwos-v1.3.0-rc1/eth-addr-reg-generator",
            h256!("0x2aa7b75eb466b754cff54463702860f8477c266c2446cea98cd26b82bf67d5dd"),
        ),
        (
            "gwos-v1.3.0-rc1/eth-addr-reg-generator.debug",
            h256!("0xeadf0d2deae3cb6512f14870d9dd13d100100b109e78d3d4c34d8098b4c5c621"),
        ),
        (
            "godwoken-polyjuice-v1.1.5-beta/generator",
            h256!("0x1829c695c91572840352794505714113025d0d55c8d318c2e013c4c2c971cf02"),
        ),
        (
            "godwoken-polyjuice-v1.1.5-beta/generator.debug",
            h256!("0x1050a2d4069f8496595e422b01dbbc04587787bd1e64e2f57a6f7b283216d7c9"),
        ),
        (
            "godwoken-polyjuice-v1.2.0/generator",
            h256!("0x1e48c135dcb6eb97a96295bbe7e9b104df05d7e48a8a115b4cfebc36f4555634"),
        ),
        (
            "godwoken-polyjuice-v1.2.0/generator.debug",
            h256!("0x1368dd12b87adaf821275c1db8a634151a51d59aeb830517139c9a80e95b15fe"),
        ),
        (
            "godwoken-polyjuice-v1.4.0/generator",
            h256!("0xa8a7868cc051af1e61e7cee267a42d2a957beac4788653e8a335d9a72f578a9b"),
        ),
        (
            "godwoken-polyjuice-v1.4.0/generator.debug",
            h256!("0x8ff1f67572b3e2533455ea7a743f7fae7de203e74c5ba8b3a9790de43a2fa4db"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator",
            h256!("0x2a42be6177fe639b72f3b93e24fa1c6e037cf80678f62a809c85fc90662daf16"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator.debug",
            h256!("0x8ff1f67572b3e2533455ea7a743f7fae7de203e74c5ba8b3a9790de43a2fa4db"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator_log",
            h256!("0xaaed6b05b36af25235c8534f5c84274c52ed6f7d8adb19641d0e8eb57972ada1"),
        ),
        (
            "godwoken-polyjuice-v1.4.1/generator_log.debug",
            h256!("0x8ff85eaaa039ca15f13f36d8ce574ca48ec39ea6b5136d99f5f83cbc9c3fe84b"),
        ),
        (
            "godwoken-polyjuice-v1.4.5/generator",
            h256!("0xee766d4a0e56a4c46f8917d52847a20bf9dcc3cd38f705faceec81dc39cdd53a"),
        ),
        (
            "godwoken-polyjuice-v1.4.5/generator.debug",
            h256!("0xd00759f32fe075dcc3078204fc37951c25efe6ea1102b1ba227d14e023e16398"),
        ),
        (
            "godwoken-polyjuice-v1.5.0/generator",
            h256!("0xf89d4eb312fa4b8df9a3acc0a25f870201106825d9a609612c61aa8263cda1b8"),
        ),
        (
            "godwoken-polyjuice-v1.5.0/generator.debug",
            h256!("0x5a67965be8a8334ec9feb27e143f8b1eb9c8e0ffff3728b1fd8903acd62e34df"),
        ),
        (
            "godwoken-polyjuice-v1.5.2/generator",
            h256!("0x9589669de5cb1b7a1b97bd5679f9e480be264a34958e4aea8f15504cab19f61d"),
        ),
        (
            "godwoken-polyjuice-v1.5.2/generator.debug",
            h256!("0x50fb7623564048dbcef97dbbd9587b284180e0cf8d5350a69bb65d6cfc24c51c"),
        ),
        (
            "godwoken-polyjuice-v1.5.3/generator",
            h256!("0x342bac1659df8b1f12201ff952efc298b17d8875bda911ca059629486338604c"),
        ),
        (
            "godwoken-polyjuice-v1.5.3/generator.debug",
            h256!("0xa2dccc4d06c011afcf985e32d3bb04b47ced54de93ee688344829f11549a784e"),
        ),
        (
            "godwoken-polyjuice-v1.5.5/generator",
            h256!("0xe4d2fed018645548204215413c13abd24ad94398ee28822c4f8f503268fefddb"),
        ),
        (
            "godwoken-polyjuice-v1.5.5/generator.debug",
            h256!("0x25e8f3889c890805bc4eaca0c564fc64c1ced7b85d4592147d679de2cdf56fd4"),
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
