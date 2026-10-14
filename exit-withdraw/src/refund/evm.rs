//! Offline V1 getter execution over authenticated DB reads. There is no commit API.
//! Polyjuice Berlin and read-only native precompiles f0/f4 are explicit; other
//! Godwoken-specific precompiles fail fatally rather than behaving as empty EOAs.
use crate::{
    db::Database as GodwokenDb,
    model::{eth_address, hx},
    v0::field,
};
use anyhow::{bail, ensure, Context, Result};
use gw_common::state::State;
use gw_types::registry_address::RegistryAddress;
use gw_types::{packed, prelude::*};
use revm::{
    primitives::{
        AccountInfo, Address, Bytecode, Bytes, PrecompileErrors, PrecompileOutput,
        PrecompileResult, SpecId, TxKind, B256, U256,
    },
    ContextPrecompile, ContextStatefulPrecompile, Database, Evm, InnerEvmContext,
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub struct FrozenDb<'a> {
    db: &'a GodwokenDb,
    ids: BTreeMap<Address, u32>,
    storage_cache: BTreeMap<(Address, U256), U256>,
    account_cache: BTreeMap<Address, Option<AccountInfo>>,
}
impl<'a> FrozenDb<'a> {
    pub fn new(db: &'a GodwokenDb, accounts: &[Value]) -> Result<Self> {
        let mut ids = BTreeMap::new();
        for a in accounts {
            if let Some(address) = a["address"].as_str() {
                let id = u32::try_from(a["account_id"].as_u64().context("Account id")?)?;
                ensure!(
                    ids.insert(Address::from(eth_address(address)?), id)
                        .is_none(),
                    "Duplicate EVM address"
                );
            }
        }
        Ok(Self {
            db,
            ids,
            storage_cache: BTreeMap::new(),
            account_cache: BTreeMap::new(),
        })
    }
    fn native_balance(&self, id: u32, address: Address) -> Result<U256> {
        let value = self
            .db
            .get_sudt_balance(id, &RegistryAddress::new(2, address.to_vec()))?;
        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);
        Ok(U256::from_be_bytes(bytes))
    }
    fn native_supply(&self, id: u32) -> Result<U256> {
        let value = self.db.get_sudt_total_supply(id)?;
        let mut bytes = [0; 32];
        value.to_big_endian(&mut bytes);
        Ok(U256::from_be_bytes(bytes))
    }
}
impl Database for FrozenDb<'_> {
    type Error = anyhow::Error;
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>> {
        if let Some(info) = self.account_cache.get(&address) {
            return Ok(info.clone());
        }
        let balance = self.native_balance(1, address)?;
        let Some(id) = self.ids.get(&address).copied() else {
            return Ok((balance != U256::ZERO).then_some(AccountInfo {
                balance,
                ..Default::default()
            }));
        };
        let mut code_key = field(id, 255);
        code_key[5] = 1;
        let code_hash = self.db.get_value(id, &code_key)?;
        let code = if code_hash == [0; 32] {
            Bytecode::default()
        } else {
            let data = self
                .db
                .data(&code_hash)?
                .context("Missing authenticated runtime")?;
            Bytecode::new_raw(data.into())
        };
        let info = Some(AccountInfo {
            balance,
            nonce: self.db.get_nonce(id)?.into(),
            code_hash: code.hash_slow(),
            code: Some(code),
        });
        self.account_cache.insert(address, info.clone());
        Ok(info)
    }
    fn code_by_hash(&mut self, _: B256) -> Result<Bytecode> {
        bail!("Runtime must be loaded from its authenticated account")
    }
    fn storage(&mut self, address: Address, index: U256) -> Result<U256> {
        if let Some(value) = self.storage_cache.get(&(address, index)) {
            return Ok(*value);
        }
        let Some(id) = self.ids.get(&address) else {
            return Ok(U256::ZERO);
        };
        let value = U256::from_be_bytes(self.db.get_value(*id, &index.to_be_bytes::<32>())?);
        self.storage_cache.insert((address, index), value);
        Ok(value)
    }
    fn block_hash(&mut self, _: u64) -> Result<B256> {
        bail!("BLOCKHASH is not supported by the reviewed getter adapter")
    }
}
struct NativePrecompile(u8);
impl<'a, 'b> ContextStatefulPrecompile<&'b mut FrozenDb<'a>> for NativePrecompile {
    fn call(
        &self,
        input: &Bytes,
        gas: u64,
        ctx: &mut InnerEvmContext<&'b mut FrozenDb<'a>>,
    ) -> PrecompileResult {
        let value = (|| -> Result<U256> {
            ensure!(gas >= 150, "Insufficient native precompile gas");
            ensure!(
                self.0 == 0xf0 || self.0 == 0xf4,
                "Unsupported or mutating Godwoken precompile"
            );
            let (id, address) = native_arguments(self.0, input)?;
            if let Some(a) = address {
                ctx.db.native_balance(id, a)
            } else {
                ctx.db.native_supply(id)
            }
        })()
        .map_err(|e| PrecompileErrors::Fatal {
            msg: format!("Frozen DB native precompile: {e:#}"),
        })?;
        Ok(PrecompileOutput::new(
            150,
            Bytes::copy_from_slice(&value.to_be_bytes::<32>()),
        ))
    }
}
fn native_arguments(kind: u8, input: &[u8]) -> Result<(u32, Option<Address>)> {
    ensure!(
        (kind == 0xf0 && input.len() == 64) || (kind == 0xf4 && input.len() == 32),
        "Native precompile input length or kind"
    );
    ensure!(input[..28] == [0; 28], "Native asset id overflow");
    let id = u32::from_be_bytes(input[28..32].try_into()?);
    let addr = if kind == 0xf0 {
        ensure!(input[32..44] == [0; 12], "Native address padding");
        Some(Address::from_slice(&input[44..64]))
    } else {
        None
    };
    Ok((id, addr))
}
/// `data` is an explicitly selected getter call. Any EVM overlay is discarded
/// after this call. The underlying adapter implements Database, never DatabaseCommit.
pub fn call(backing: &mut FrozenDb<'_>, target: [u8; 20], data: &[u8]) -> Result<Vec<u8>> {
    let db = backing.db;
    let block_bytes = db
        .tree
        .store()
        .get(
            crate::schema::COLUMN_BLOCK,
            &crate::model::hash32(&db.tip.hash)?,
        )?
        .context("Missing frozen tip block")?;
    let block = packed::L2Block::from_slice(&block_bytes)?;
    ensure!(
        crate::model::hash(block.raw().as_slice()) == crate::model::hash32(&db.tip.hash)?,
        "Frozen header hash mismatch"
    );
    let producer = RegistryAddress::from_slice(&block.raw().block_producer().raw_data())
        .context("Frozen block producer")?;
    ensure!(
        producer.registry_id == 2 && producer.address.len() == 20,
        "Unsupported block producer registry"
    );
    let coinbase = Address::from_slice(&producer.address);

    let mut vm = Evm::builder()
        .with_db(backing)
        .with_spec_id(SpecId::BERLIN)
        .modify_cfg_env(|cfg| {
            cfg.chain_id = 71402;
            cfg.memory_limit = 3 * 1024 * 1024;
        })
        .modify_block_env(|block| {
            block.coinbase = coinbase;
            block.number = U256::from(db.tip.number);
            block.timestamp = U256::from(db.tip.timestamp / 1000);
            block.gas_limit = U256::from(12_500_000);
            block.basefee = U256::ZERO;
            block.difficulty = U256::from(2_500_000_000_000_000u64);
        })
        .modify_tx_env(|tx| {
            tx.caller = Address::ZERO;
            tx.transact_to = TxKind::Call(Address::from(target));
            tx.data = Bytes::copy_from_slice(data);
            tx.gas_limit = 12_500_000;
            tx.gas_price = U256::ZERO;
            tx.value = U256::ZERO;
            tx.nonce = None;
            tx.chain_id = Some(71402);
        })
        .append_handler_register(|handler| {
            let defaults = handler.pre_execution.load_precompiles();
            handler.pre_execution.load_precompiles = Arc::new(move || {
                let mut precompiles = defaults.clone();
                for code in 0xf0..=0xff {
                    let mut address = [0; 20];
                    address[19] = code;
                    precompiles.extend([(
                        Address::from(address),
                        ContextPrecompile::ContextStateful(Arc::new(NativePrecompile(code))),
                    )]);
                }
                precompiles
            });
        })
        .build();
    let result = vm
        .transact()
        .map_err(|e| anyhow::anyhow!("Offline EVM getter failed: {e:?}"))?;
    ensure!(
        result.result.is_success(),
        "Offline getter reverted or halted at {}: {:?}",
        hx(target),
        result.result
    );
    let output = result.result.output().context("Missing getter output")?;
    ensure!(
        output.len() <= 25 * 1024,
        "Getter exceeds Polyjuice return limit"
    );
    Ok(output.to_vec())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_precompile_domain_and_padding() {
        let mut bytes = [0; 64];
        bytes[31] = 128;
        bytes[63] = 9;
        assert_eq!(native_arguments(0xf0, &bytes).unwrap().0, 128);
        assert!(native_arguments(0xf1, &bytes).is_err());
        assert!(native_arguments(0xf4, &bytes).is_err());
        bytes[32] = 1;
        assert!(native_arguments(0xf0, &bytes).is_err());
        bytes[0] = 1;
        assert!(native_arguments(0xf4, &bytes[..32]).is_err());
    }
}
