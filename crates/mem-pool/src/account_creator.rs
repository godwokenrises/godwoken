use anyhow::{anyhow, bail, Result};

use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    ckb_decimal::CKBCapacity,
    registry_address::RegistryAddress,
    state::State,
};
use gw_generator::account_lock_manage::secp256k1::Secp256k1Eth;
use gw_types::{
    core::{AllowedEoaType, ScriptHashType},
    h256::*,
    packed::{
        BatchCreateEthAccounts, Fee, L2Transaction, LogItemReader, MetaContractArgs,
        RawL2Transaction, Script, ScriptVec,
    },
    prelude::{Builder, Entity, Pack, Reader, Unpack},
    U256,
};
use gw_utils::{
    script_log::{parse_log, GwLog, GW_LOG_SUDT_TRANSFER},
    wallet::Wallet,
    RollupContext,
};
use tracing::instrument;

const META_CONTRACT_ACCOUNT_ID: u32 = RESERVED_ACCOUNT_ID;
const ONE_CKB: u64 = 10u64.pow(8);
pub const MIN_BALANCE: u64 = ONE_CKB;

pub fn filter_new_address<'a>(
    logs: impl IntoIterator<Item = LogItemReader<'a>>,
    state: &impl State,
) -> Option<Vec<RegistryAddress>> {
    let do_filter = |log: LogItemReader<'_>| -> Result<Option<RegistryAddress>> {
        if (GW_LOG_SUDT_TRANSFER, CKB_SUDT_ACCOUNT_ID)
            != (log.service_flag().into(), log.account_id().unpack())
        {
            return Ok(None);
        }

        let to = match parse_log(&log.to_entity())? {
            GwLog::SudtTransfer { to_address, .. } => to_address,
            _ => return Ok(None),
        };

        if state.get_script_hash_by_registry_address(&to)?.is_some() {
            return Ok(None);
        }

        let min_balance: U256 = CKBCapacity::from_layer1(MIN_BALANCE).to_layer2();
        let to_balance = state.get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &to)?;
        if to_balance < min_balance {
            tracing::info!("new address {:?} balance less than {}", to, min_balance);
            return Ok(None);
        }

        Ok(Some(to))
    };

    let filtered = logs.into_iter().filter_map(|log| match do_filter(log) {
        Err(err) => {
            tracing::error!("parse log {}", err);
            None
        }
        Ok(new) => new,
    });

    let new: Vec<_> = filtered.collect();
    if new.is_empty() {
        None
    } else {
        Some(new)
    }
}

pub struct AccountCreator {
    pub chain_id: u64,
    pub rollup_script_hash: H256,
    pub eth_lock_code_hash: H256,
    pub creator_script_hash: H256,
    pub creator_wallet: Wallet,
}

impl AccountCreator {
    pub const MAX_CREATE_ACCOUNTS_PER_BATCH: usize = 50;

    pub fn create(rollup_context: &RollupContext, creator_wallet: Wallet) -> Result<Self> {
        let chain_id = rollup_context.rollup_config.chain_id().unpack();
        let rollup_script_hash = rollup_context.rollup_script_hash;
        let eth_lock_code_hash = {
            let allowed_eoa_type_hashes = rollup_context.rollup_config.allowed_eoa_type_hashes();
            { allowed_eoa_type_hashes.as_reader().iter() }
                .find_map(|type_hash| {
                    if type_hash.type_().to_entity() == AllowedEoaType::Eth.into() {
                        Some(type_hash.hash().unpack())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow!("eth lock code hash not found"))?
        };

        let creator_script_hash = {
            let s = creator_wallet.eth_lock_script(&rollup_script_hash, &eth_lock_code_hash)?;
            s.hash()
        };

        let creator = Self {
            chain_id,
            rollup_script_hash,
            eth_lock_code_hash,
            creator_script_hash,
            creator_wallet,
        };

        Ok(creator)
    }

    #[instrument(skip_all)]
    pub fn build_batch_create_tx<'a>(
        &'a self,
        state: &'a impl State,
        addresses: impl IntoIterator<Item = RegistryAddress>,
    ) -> Result<Option<(L2Transaction, Vec<RegistryAddress>)>> {
        let creator_account_id = state
            .get_account_id_by_script_hash(&self.creator_script_hash)?
            .ok_or_else(|| anyhow!("creator account id not found"))?;

        let creator_registry_address = match state.get_registry_address_by_script_hash(
            ETH_REGISTRY_ACCOUNT_ID,
            &self.creator_script_hash,
        )? {
            Some(addr) => addr,
            None => bail!("creator eth registry address not found"),
        };

        let nonce = state.get_nonce(creator_account_id)?;
        let meta_contract_script_hash = state.get_script_hash(META_CONTRACT_ACCOUNT_ID)?;

        let new_addrs: Vec<_> = {
            addresses.into_iter().filter_map(|addr| {
                match state.get_script_hash_by_registry_address(&addr) {
                    Ok(None) => Some(addr),
                    Ok(Some(_)) => None,
                    Err(err) => {
                        tracing::error!("query address {:?} script hash error {}", addr, err);
                        None
                    }
                }
            })
        }
        .collect();

        let create_accounts = { new_addrs.iter() }
            .take(Self::MAX_CREATE_ACCOUNTS_PER_BATCH)
            .collect::<Vec<_>>();
        if create_accounts.is_empty() {
            return Ok(None);
        }
        tracing::info!("create account {:?}", create_accounts);

        let create_accounts = { create_accounts.into_iter() }
            .map(|a| self.to_account_script(a))
            .collect::<Vec<_>>();
        let next_batch = { new_addrs.into_iter() }
            .skip(Self::MAX_CREATE_ACCOUNTS_PER_BATCH)
            .collect::<Vec<_>>();

        let fee = Fee::new_builder()
            .registry_id(ETH_REGISTRY_ACCOUNT_ID.pack())
            .amount(0u128.pack())
            .build();
        let batch_create = BatchCreateEthAccounts::new_builder()
            .fee(fee)
            .scripts(ScriptVec::new_builder().set(create_accounts).build())
            .build();
        let args = MetaContractArgs::new_builder().set(batch_create).build();

        let raw_l2tx = RawL2Transaction::new_builder()
            .chain_id(self.chain_id.pack())
            .from_id(creator_account_id.pack())
            .to_id(META_CONTRACT_ACCOUNT_ID.pack())
            .nonce(nonce.pack())
            .args(args.as_bytes().pack())
            .build();

        let signing_message = Secp256k1Eth::eip712_signing_message(
            self.chain_id,
            &raw_l2tx,
            creator_registry_address,
            meta_contract_script_hash,
        )?;
        let sign = self.creator_wallet.sign_message(signing_message)?;

        let tx = L2Transaction::new_builder()
            .raw(raw_l2tx)
            .signature(sign.pack())
            .build();

        Ok(Some((tx, next_batch)))
    }

    fn to_account_script(&self, registry_address: &RegistryAddress) -> Script {
        let mut args = self.rollup_script_hash.as_slice().to_vec();
        args.extend_from_slice(&registry_address.address);

        Script::new_builder()
            .code_hash(self.eth_lock_code_hash.pack())
            .hash_type(ScriptHashType::Type.into())
            .args(args.pack())
            .build()
    }
}
