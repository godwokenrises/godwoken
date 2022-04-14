use crate::godwoken_rpc::GodwokenRpcClient;
use crate::hasher::CkbHasher;
use crate::types::ScriptsDeploymentResult;
use anyhow::{anyhow, Result};
use ckb_crypto::secp::Privkey;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_sdk::SECP256K1;
use ckb_types::{
    bytes::Bytes as CKBBytes, core::ScriptHashType, prelude::Builder as CKBBuilder,
    prelude::Entity as CKBEntity,
};
use gw_types::{
    bytes::Bytes as GwBytes,
    packed::{Byte32, Script},
    prelude::Pack as GwPack,
};
use sha3::{Digest, Keccak256};
use std::str::FromStr;
use std::{fs, path::Path};

pub fn privkey_to_eth_address(privkey: &H256) -> Result<CKBBytes> {
    let privkey = secp256k1::SecretKey::from_slice(privkey.as_bytes())
        .map_err(|err| anyhow!("Invalid secp256k1 secret key format, error: {}", err))?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&SECP256K1, &privkey);
    let pubkey_hash = {
        let mut hasher = Keccak256::new();
        hasher.update(&pubkey.serialize_uncompressed()[1..]);
        let buf = hasher.finalize();
        let mut pubkey_hash = [0u8; 20];
        pubkey_hash.copy_from_slice(&buf[12..]);
        pubkey_hash
    };
    let s = CKBBytes::from(pubkey_hash.to_vec());
    Ok(s)
}

fn sign_message(msg: &H256, privkey_data: H256) -> Result<[u8; 65]> {
    let privkey = Privkey::from(privkey_data);
    let signature = privkey.sign_recoverable(msg)?;
    let mut inner = [0u8; 65];
    inner.copy_from_slice(&signature.serialize());
    Ok(inner)
}

pub fn eth_sign(msg: &H256, privkey: H256) -> Result<[u8; 65]> {
    let mut signature = sign_message(msg, privkey)?;
    let v = &mut signature[64];
    if *v >= 27 {
        *v -= 27;
    }
    Ok(signature)
}

pub fn privkey_to_l2_script_hash(
    privkey: &H256,
    rollup_type_hash: &H256,
    scripts_deployment: &ScriptsDeploymentResult,
) -> Result<H256> {
    let eth_address = privkey_to_eth_address(privkey)?;

    let code_hash = Byte32::from_slice(
        scripts_deployment
            .eth_account_lock
            .script_type_hash
            .as_bytes(),
    )?;

    let mut args_vec = rollup_type_hash.as_bytes().to_vec();
    args_vec.append(&mut eth_address.to_vec());
    let args = GwPack::pack(&GwBytes::from(args_vec));

    let script = Script::new_builder()
        .code_hash(code_hash)
        .hash_type(ScriptHashType::Type.into())
        .args(args)
        .build();

    let script_hash = CkbHasher::new().update(script.as_slice()).finalize();

    Ok(script_hash)
}

pub fn l2_script_hash_to_short_script_hash(script_hash: &H256) -> GwBytes {
    let short_script_hash = &script_hash.as_bytes()[..20];

    GwBytes::from(short_script_hash.to_vec())
}

pub fn privkey_to_short_script_hash(
    privkey: &H256,
    rollup_type_hash: &H256,
    scripts_deployment: &ScriptsDeploymentResult,
) -> Result<GwBytes> {
    let script_hash = privkey_to_l2_script_hash(privkey, rollup_type_hash, scripts_deployment)?;

    let short_script_hash = l2_script_hash_to_short_script_hash(&script_hash);
    Ok(short_script_hash)
}

pub async fn short_script_hash_to_account_id(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    short_script_hash: &GwBytes,
) -> Result<Option<u32>> {
    let bytes = JsonBytes::from_bytes(short_script_hash.clone());
    let script_hash = match godwoken_rpc_client
        .get_script_hash_by_short_script_hash(bytes)
        .await?
    {
        Some(h) => h,
        None => {
            return Err(anyhow!(
                "script hash by short script hash: 0x{} not found",
                hex::encode(short_script_hash.to_vec()),
            ))
        }
    };
    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(script_hash)
        .await?;

    Ok(account_id)
}

// address: 0x... / id: 1
pub async fn parse_account_short_script_hash(
    godwoken: &mut GodwokenRpcClient,
    account: &str,
) -> Result<GwBytes> {
    // if match short script hash
    if account.starts_with("0x") && account.len() == 42 {
        let r = GwBytes::from(hex::decode(account[2..].as_bytes())?);
        return Ok(r);
    }

    // if match id
    let account_id: u32 = match account.parse() {
        Ok(a) => a,
        Err(_) => return Err(anyhow!("account id parse error!")),
    };
    let script_hash = godwoken.get_script_hash(account_id).await?;
    let short_script_hash = GwBytes::from((&script_hash.as_bytes()[..20]).to_vec());
    Ok(short_script_hash)
}

pub fn read_privkey(privkey_path: &Path) -> Result<H256> {
    let privkey_string = fs::read_to_string(privkey_path)?
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("Privkey file is empty"))?;
    let privkey = H256::from_str(privkey_string.trim().trim_start_matches("0x"))?;
    Ok(privkey)
}
