use std::convert::TryInto;

use anyhow::{anyhow, Result};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    registry_address::RegistryAddress,
    state::State,
    H256,
};
use gw_generator::traits::StateExt;
use gw_store::traits::chain_store::ChainStore;
use gw_types::{
    core::ScriptHashType,
    packed::{LogItem, Script},
    prelude::{Builder, Entity, Pack},
};
use gw_utils::script_log::{parse_log, GwLog};

use super::chain::{TestChain, POLYJUICE_VALIDATOR_CODE_HASH};

pub mod erc20;

pub struct PolyjuiceAccount {
    pub id: u32,
}

impl PolyjuiceAccount {
    pub fn create(rollup_script_hash: H256, state: &mut impl StateExt) -> Result<Self> {
        let polyjuice_script = {
            let mut args = rollup_script_hash.as_slice().to_vec();
            args.extend_from_slice(&CKB_SUDT_ACCOUNT_ID.to_le_bytes());

            Script::new_builder()
                .code_hash(POLYJUICE_VALIDATOR_CODE_HASH.pack())
                .hash_type(ScriptHashType::Type.into())
                .args(args.pack())
                .build()
        };

        let id = state.create_account_from_script(polyjuice_script)?;

        Ok(Self { id })
    }
}

pub struct PolyjuiceArgsBuilder {
    args: Vec<u8>,
    data: Vec<u8>,
}

impl Default for PolyjuiceArgsBuilder {
    fn default() -> Self {
        let mut args = vec![0u8; 52];
        args[0..7].copy_from_slice(b"\xFF\xFF\xFFPOLY");

        let builder = PolyjuiceArgsBuilder { args, data: vec![] };

        builder.gas_limit(21000).gas_price(1).value(0)
    }
}

impl PolyjuiceArgsBuilder {
    pub fn create(mut self, create: bool) -> Self {
        if create {
            self.args[7] = 3;
        } else {
            self.args[7] = 0;
        }
        self
    }

    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.args[8..16].copy_from_slice(&gas_limit.to_le_bytes());
        self
    }

    pub fn gas_price(mut self, gas_price: u128) -> Self {
        self.args[16..32].copy_from_slice(&gas_price.to_le_bytes());
        self
    }

    pub fn value(mut self, value: u128) -> Self {
        self.args[32..48].copy_from_slice(&value.to_le_bytes());
        self
    }

    pub fn data(mut self, data: Vec<u8>) -> Self {
        let data_len: u32 = data.len().try_into().unwrap();
        self.args[48..52].copy_from_slice(&data_len.to_le_bytes());
        self.data = data;
        self
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.args.extend(self.data);
        self.args
    }
}

pub struct PolyjuiceSystemLog {
    pub created_address: [u8; 20],
    pub status_code: u32,
}

impl PolyjuiceSystemLog {
    pub fn parse_from_tx_hash(chain: &TestChain, tx_hash: H256) -> Result<Self> {
        let receipt = chain
            .store()
            .get_mem_pool_transaction_receipt(&tx_hash)?
            .ok_or_else(|| anyhow!("tx receipt not found"))?;

        Self::parse_logs(receipt.logs())
    }

    pub fn parse_logs(logs: impl IntoIterator<Item = LogItem>) -> Result<Self> {
        let (created_address, status_code) = logs
            .into_iter()
            .filter_map(|item| parse_log(&item).ok())
            .find_map(|gw_log| match gw_log {
                GwLog::PolyjuiceSystem {
                    created_address,
                    status_code,
                    ..
                } => Some((created_address, status_code)),
                _ => None,
            })
            .ok_or_else(|| anyhow!("polyjuice system log not found"))?;

        let system_log = PolyjuiceSystemLog {
            created_address,
            status_code,
        };

        Ok(system_log)
    }

    pub fn contract_account_id(&self, state: &impl State) -> Result<u32> {
        let registry_address =
            RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, self.created_address.to_vec());
        let script_hash = state
            .get_script_hash_by_registry_address(&registry_address)?
            .ok_or_else(|| anyhow!("script hash not found"))?;

        let account_id = state
            .get_account_id_by_script_hash(&script_hash)?
            .ok_or_else(|| anyhow!("contract account id not found"))?;

        Ok(account_id)
    }
}

pub fn abi_encode_eth_address(registry_address: &RegistryAddress) -> [u8; 32] {
    assert_eq!(registry_address.address.len(), 20);

    let mut buf = [0u8; 32];
    buf[12..32].copy_from_slice(&registry_address.address);

    buf
}
