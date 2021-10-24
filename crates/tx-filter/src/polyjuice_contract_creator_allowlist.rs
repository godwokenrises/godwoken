use anyhow::{anyhow, Result};
use dashmap::DashSet;
use gw_common::{state::State, H256};
use gw_store::state_db::{CheckPoint, StateDBMode, StateDBTransaction, SubState};
use gw_store::Store;
use gw_traits::CodeStore;
use gw_types::bytes::Bytes;
use gw_types::{packed::L2Transaction, prelude::Unpack};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Permission denied, not allowed to create contract")]
    PermissionDenied,
}

pub struct PolyjuiceArgs;

impl PolyjuiceArgs {
    // https://github.com/nervosnetwork/godwoken-polyjuice/blob/v0.6.0-rc1/polyjuice-tests/src/helper.rs#L322
    pub fn is_contract_create(args: &[u8]) -> bool {
        args[7] == 3u8
    }
}

pub struct PolyjuiceContractCreatorAllowList {
    pub polyjuice_deployment_ids: DashSet<u32>,
    pub polyjuice_code_hash: H256,
    pub allowed_creator_ids: Vec<u32>,
    pub store: Store,
}

impl PolyjuiceContractCreatorAllowList {
    pub fn new(polyjuice_code_hash: H256, allowed_creator_ids: Vec<u32>, store: Store) -> Self {
        Self {
            polyjuice_deployment_ids: DashSet::new(),
            polyjuice_code_hash,
            allowed_creator_ids,
            store,
        }
    }

    pub fn validate(&self, tx: &L2Transaction) -> Result<()> {
        let raw_tx = tx.raw();
        let to_id: u32 = raw_tx.to_id().unpack();
        let from_id: u32 = raw_tx.from_id().unpack();
        let is_contract_create =
            PolyjuiceArgs::is_contract_create(&Unpack::<Bytes>::unpack(&raw_tx.args()));

        if is_contract_create && self.polyjuice_deployment_ids.contains(&to_id) {
            if self.allowed_creator_ids.contains(&from_id) {
                return Ok(());
            } else {
                return Err(Error::PermissionDenied.into());
            }
        }

        let to_script = {
            let db = self.store.begin_transaction();
            let tip_block_number = db.get_tip_block()?.raw().number().unpack();
            let state_db = StateDBTransaction::from_checkpoint(
                &db,
                CheckPoint::new(tip_block_number, SubState::MemBlock),
                StateDBMode::ReadOnly,
            )?;

            let state = state_db.state_tree()?;
            let script_hash = state.get_script_hash(to_id)?;
            state
                .get_script(&script_hash)
                .ok_or_else(|| anyhow!("unknown to_id"))?
        };

        if Unpack::<H256>::unpack(&to_script.code_hash()) != self.polyjuice_code_hash {
            return Ok(());
        }

        self.polyjuice_deployment_ids.insert(to_id);
        if is_contract_create && !self.allowed_creator_ids.contains(&from_id) {
            return Err(Error::PermissionDenied.into());
        }

        Ok(())
    }
}
