use gw_common::state::State;
use gw_common::H256;
use gw_config::RPCConfig;
use gw_traits::CodeStore;
use gw_types::bytes::Bytes;
use gw_types::packed::RawL2Transaction;
use gw_types::prelude::Unpack;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Permission denied, cannot create polyjuice contract from account {account_id}")]
    PermissionDenied { account_id: u32 },
    #[error("{0}")]
    Common(gw_common::error::Error),
}

impl From<gw_common::error::Error> for Error {
    fn from(err: gw_common::error::Error) -> Self {
        Error::Common(err)
    }
}

pub struct PolyjuiceContractCreatorAllowList {
    pub polyjuice_code_hash: H256,
    pub allowed_creator_ids: Vec<u32>,
}

impl PolyjuiceContractCreatorAllowList {
    pub fn from_rpc_config(config: &RPCConfig) -> Option<Self> {
        match (
            &config.allowed_polyjuice_contract_creator_account_ids,
            &config.polyjuice_script_code_hash,
        ) {
            (Some(allowed_creator_ids), Some(polyjuice_code_hash)) => Some(Self::new(
                H256::from(polyjuice_code_hash.0),
                allowed_creator_ids.to_vec(),
            )),
            _ => None,
        }
    }

    pub fn new(polyjuice_code_hash: H256, allowed_creator_ids: Vec<u32>) -> Self {
        Self {
            polyjuice_code_hash,
            allowed_creator_ids,
        }
    }

    // TODO: Cached polyjuice deployment id? But tx may fail then invalid id.
    pub fn validate_with_state<S: State + CodeStore>(
        &self,
        state: &S,
        tx: &RawL2Transaction,
    ) -> Result<(), Error> {
        let to_id: u32 = tx.to_id().unpack();

        // 0 is reversed for meta contract and 1 is reversed for sudt
        if to_id < 2 {
            return Ok(());
        }

        let script_hash = state.get_script_hash(to_id)?;
        let to_script = state
            .get_script(&script_hash)
            .ok_or(gw_common::error::Error::MissingKey)?;

        if Unpack::<H256>::unpack(&to_script.code_hash()) != self.polyjuice_code_hash {
            return Ok(());
        }

        let from_id: u32 = tx.from_id().unpack();
        let is_contract_create =
            PolyjuiceArgs::is_contract_create(&Unpack::<Bytes>::unpack(&tx.args()));

        if is_contract_create && !self.allowed_creator_ids.contains(&from_id) {
            return Err(Error::PermissionDenied {
                account_id: from_id,
            });
        }

        Ok(())
    }
}

struct PolyjuiceArgs;

impl PolyjuiceArgs {
    // https://github.com/nervosnetwork/godwoken-polyjuice/blob/v0.6.0-rc1/polyjuice-tests/src/helper.rs#L322
    fn is_contract_create(args: &[u8]) -> bool {
        args[7] == 3u8
    }
}
