use gw_tools::deposit_ckb;
use std::path::Path;

pub const CKB_RPC_URL: &str = "http://127.0.0.1:8114";

pub fn deposit(
    privkey_path: &Path,
    deployment_results_path: &Path,
    config_path: &Path,
    times: u32,
) -> Result<(), String> {
    log::info!("[test mode contro]: deposit");

    for _ in 0..times {
        deposit_ckb::deposit_ckb_to_layer1(
            privkey_path,
            deployment_results_path,
            config_path,
            "10000",
            "0.0001",
            CKB_RPC_URL,
            None,
        )?;
    }

    Ok(())
}
