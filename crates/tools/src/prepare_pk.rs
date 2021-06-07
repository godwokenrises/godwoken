use anyhow::Result;
use hex;
use rand::Rng;
use std::path::Path;

pub fn prepare_pk(
    _privkey_path: &Path,
    _ckb_rpc_url: &str,
    _ckb_count: u32,
    nodes_count: u32,
    _output_dir: &Path,
    _poa_config_path: &Path,
    _rollup_config_path: &Path,
) -> Result<()> {
    generate_private_keys(nodes_count);
    get_wallet_info();
    transfer_ckb();
    generate_poa_config();
    generate_rollup_config();
    Ok(())
}

fn generate_private_keys(nodes_count: u32) -> Vec<String> {
    let private_keys: Vec<String> = (0..nodes_count)
        .map(|_| {
            let key = rand::thread_rng().gen::<[u8; 32]>();
            format!("0x{}", hex::encode(key))
        })
        .collect();
    private_keys
}

fn get_wallet_info() {}

fn transfer_ckb() {}

fn generate_poa_config() {}

fn generate_rollup_config() {}
