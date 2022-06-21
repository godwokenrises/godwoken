use anyhow::{anyhow, bail, Result};
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, state::State, H256,
};
use gw_generator::account_lock_manage::{secp256k1::Secp256k1Eth, LockAlgorithm};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{RawL2Transaction, Script},
    prelude::{Builder, Entity, Pack, Unpack},
};
use tracing::instrument;

pub mod eoa_creator;

#[instrument(skip_all)]
pub fn recover_registry_address(
    chain_id: u64,
    state: &(impl State + CodeStore),
    raw_tx: &RawL2Transaction,
    signature: &Bytes,
) -> Result<RegistryAddress> {
    if Unpack::<u32>::unpack(&raw_tx.from_id()) != 0 {
        bail!("from id isnt 0");
    }
    if raw_tx.chain_id().unpack() != chain_id {
        bail!("mismatch chain id");
    }

    let to_script_hash = state.get_script_hash(raw_tx.to_id().unpack())?;
    if to_script_hash.is_zero() {
        bail!("to script hash is zero");
    }

    let to_script = state
        .get_script(&to_script_hash)
        .ok_or_else(|| anyhow!("to script not found"))?;

    let message = Secp256k1Eth::polyjuice_tx_signing_message(chain_id, raw_tx, &to_script)?;
    let address = Secp256k1Eth::default().recover(message, signature)?;

    Ok(RegistryAddress::new(
        ETH_REGISTRY_ACCOUNT_ID,
        address.to_vec(),
    ))
}

pub fn to_eth_eoa_script(
    rollup_script_hash: H256,
    eth_lock_code_hash: H256,
    registry_address: &RegistryAddress,
) -> Script {
    let mut args = rollup_script_hash.as_slice().to_vec();
    args.extend_from_slice(&registry_address.address);

    Script::new_builder()
        .code_hash(eth_lock_code_hash.pack())
        .hash_type(ScriptHashType::Type.into())
        .args(args.pack())
        .build()
}
