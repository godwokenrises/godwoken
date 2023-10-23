use anyhow::anyhow;
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, state::State,
};
use gw_generator::account_lock_manage::{secp256k1::Secp256k1Eth, LockAlgorithm};
use gw_traits::CodeStore;
use gw_types::{
    bytes::Bytes,
    h256::*,
    packed::{L2Transaction, RawL2Transaction, Script},
    prelude::*,
};
use tracing::instrument;

use super::{error::PolyjuiceTxSenderRecoverError, eth_recover::EthAccountContext};

pub type EthEOAScript = Script;

pub enum PolyjuiceTxEthSender {
    New {
        registry_address: RegistryAddress,
        account_script: EthEOAScript,
    },
    Exist {
        registry_address: RegistryAddress,
        account_id: u32,
    },
}

impl PolyjuiceTxEthSender {
    #[instrument(skip_all)]
    pub fn recover(
        ctx: &EthAccountContext,
        state: &(impl State + CodeStore),
        tx: &L2Transaction,
    ) -> Result<Self, PolyjuiceTxSenderRecoverError> {
        let sig = tx.signature().unpack();

        let registry_address = recover_registry_address(ctx, state, &tx.raw(), &sig)?;
        let account_script = ctx.to_account_script(&registry_address);

        match state.get_script_hash_by_registry_address(&registry_address)? {
            Some(script_hash) if script_hash != account_script.hash() => {
                Err(PolyjuiceTxSenderRecoverError::DifferentScript {
                    registry_address,
                    script_hash,
                })
            }
            Some(account_script_hash) => {
                match state.get_account_id_by_script_hash(&account_script_hash)? {
                    Some(account_id) => Ok(Self::Exist {
                        registry_address,
                        account_id,
                    }),
                    None => Err(PolyjuiceTxSenderRecoverError::Internal(anyhow!(
                        "{:x} account id not found",
                        registry_address.address.pack()
                    ))),
                }
            }
            None => Ok(Self::New {
                registry_address,
                account_script,
            }),
        }
    }

    pub fn registry_address(&self) -> &RegistryAddress {
        match self {
            PolyjuiceTxEthSender::New {
                registry_address, ..
            } => registry_address,
            PolyjuiceTxEthSender::Exist {
                registry_address, ..
            } => registry_address,
        }
    }
}

#[instrument(skip_all)]
fn recover_registry_address(
    ctx: &EthAccountContext,
    state: &(impl State + CodeStore),
    raw_tx: &RawL2Transaction,
    signature: &Bytes,
) -> Result<RegistryAddress, PolyjuiceTxSenderRecoverError> {
    if raw_tx.is_chain_id_protected() && raw_tx.chain_id().unpack() != ctx.chain_id {
        return Err(PolyjuiceTxSenderRecoverError::InvalidChainId);
    }

    let to_id: u32 = raw_tx.to_id().unpack();
    let to_script_hash = state.get_script_hash(to_id).map_err(|err| {
        let tx_hash = raw_tx.hash().pack();
        log::error!("get {:x} to {} script hash {}", tx_hash, to_id, err);
        PolyjuiceTxSenderRecoverError::Internal(err.into())
    })?;
    if to_script_hash.is_zero() {
        return Err(PolyjuiceTxSenderRecoverError::ToScriptNotFound);
    }

    let to_script = state
        .get_script(&to_script_hash)
        .ok_or(PolyjuiceTxSenderRecoverError::ToScriptNotFound)?;
    if Unpack::<H256>::unpack(&to_script.code_hash()) != ctx.polyjuice_validator_code_hash {
        return Err(PolyjuiceTxSenderRecoverError::NotPolyjuiceTx);
    }

    let message = Secp256k1Eth::polyjuice_tx_signing_message(ctx.chain_id, raw_tx, &to_script)
        .map_err(PolyjuiceTxSenderRecoverError::InvalidSignature)?;
    let eth_address = Secp256k1Eth
        .recover(message, signature)
        .map_err(|err| PolyjuiceTxSenderRecoverError::InvalidSignature(err.into()))?;
    let registry_address = RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, eth_address.to_vec());

    Ok(registry_address)
}
