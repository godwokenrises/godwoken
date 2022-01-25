use gw_common::state::State;
use gw_common::H256;
use gw_config::RPCConfig;
use gw_traits::CodeStore;
use gw_types::bytes::Bytes;
use gw_types::packed::RawL2Transaction;
use gw_types::prelude::Unpack;

use std::collections::HashSet;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Permission denied, cannot create polyjuice contract from account {account_id}")]
    PermissionDenied { account_id: u32 },
    #[error("{0}")]
    Common(gw_common::error::Error),
    #[error("Script hash not found")]
    ScriptHashNotFound,
}

impl From<gw_common::error::Error> for Error {
    fn from(err: gw_common::error::Error) -> Self {
        Error::Common(err)
    }
}

#[derive(Clone)]
pub struct PolyjuiceContractCreatorAllowList {
    pub polyjuice_code_hash: H256,
    pub allowed_creator_eth_address: HashSet<[u8; 20]>,
}

impl PolyjuiceContractCreatorAllowList {
    pub fn from_rpc_config(config: &RPCConfig) -> Option<Self> {
        match (
            &config.allowed_polyjuice_contract_creator_address,
            &config.polyjuice_script_code_hash,
        ) {
            (Some(allowed_creator_address), Some(polyjuice_code_hash)) => Some(Self::new(
                H256::from(polyjuice_code_hash.0),
                allowed_creator_address
                    .iter()
                    .map(|address| address.0)
                    .collect(),
            )),
            _ => None,
        }
    }

    pub fn new(polyjuice_code_hash: H256, allowed_creator_eth_address: HashSet<[u8; 20]>) -> Self {
        Self {
            polyjuice_code_hash,
            allowed_creator_eth_address,
        }
    }

    // TODO: Cached polyjuice deployment id? But tx may fail then invalid id.
    pub fn validate_with_state<S: State + CodeStore>(
        &self,
        state: &S,
        tx: &RawL2Transaction,
    ) -> Result<(), Error> {
        let from_id: u32 = tx.from_id().unpack();
        let to_id: u32 = tx.to_id().unpack();

        // 1 is reversed for CKB sudt
        if to_id == 1 {
            return Ok(());
        }

        // check allowed list
        {
            let script_hash = state.get_script_hash(from_id)?;
            let args: Bytes = state
                .get_script(&script_hash)
                .ok_or(Error::ScriptHashNotFound)?
                .args()
                .unpack();

            // check if the args is a valid eth lock's args
            if args.len() == 52 {
                // get sender eth address
                let mut sender_address = [0u8; 20];
                sender_address.copy_from_slice(&args[32..]);

                // return ok if sender is in the allowed list
                if self.allowed_creator_eth_address.contains(&sender_address) {
                    return Ok(());
                }
            }
        };

        // create contract through meta
        let is_meta_create = to_id == 0;

        // create contract through polyjuice
        let is_polyjuice_create = {
            let script_hash = state.get_script_hash(to_id)?;
            let to_script = state
                .get_script(&script_hash)
                .ok_or(gw_common::error::Error::MissingKey)?;
            Unpack::<H256>::unpack(&to_script.code_hash()) == self.polyjuice_code_hash
                && PolyjuiceArgs::is_contract_create(&Unpack::<Bytes>::unpack(&tx.args()))
        };

        if is_meta_create || is_polyjuice_create {
            return Err(Error::PermissionDenied {
                account_id: from_id,
            });
        }

        Ok(())
    }
}

struct PolyjuiceArgs;

impl PolyjuiceArgs {
    // evmc_call_kind.EVMC_CREATE = 3
    // https://github.com/ethereum/evmc/blob/v9.0.0/include/evmc/evmc.h#L81
    // https://github.com/nervosnetwork/godwoken-polyjuice/blob/v0.6.0-rc1/polyjuice-tests/src/helper.rs#L322
    fn is_contract_create(args: &[u8]) -> bool {
        args[7] == 3u8
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::iter::FromIterator;

    use gw_common::error::Error;
    use gw_common::smt::SMT;
    use gw_common::sparse_merkle_tree::default_store::DefaultStore;
    use gw_common::state::State;
    use gw_common::H256;
    use gw_traits::CodeStore;
    use gw_types::bytes::Bytes;
    use gw_types::core::ScriptHashType;
    use gw_types::packed::{RawL2Transaction, Script};
    use gw_types::prelude::{Builder, Entity, Pack};

    use super::PolyjuiceContractCreatorAllowList;

    const TEST_POLYJUICE_SCRIPT_CODE_HASH: [u8; 32] = [1u8; 32];
    const TEST_ETH_SCRIPT_CODE_HASH: [u8; 32] = [2u8; 32];

    #[derive(Default)]
    pub struct DummyState {
        tree: SMT<DefaultStore<H256>>,
        account_count: u32,
        scripts: HashMap<H256, Script>,
        codes: HashMap<H256, Bytes>,
    }

    impl State for DummyState {
        fn get_raw(&self, key: &H256) -> Result<H256, Error> {
            let v = self.tree.get(key)?;
            Ok(v)
        }
        fn update_raw(&mut self, key: H256, value: H256) -> Result<(), Error> {
            self.tree.update(key, value)?;
            Ok(())
        }
        fn calculate_root(&self) -> Result<H256, Error> {
            let root = *self.tree.root();
            Ok(root)
        }
        fn get_account_count(&self) -> Result<u32, Error> {
            Ok(self.account_count)
        }
        fn set_account_count(&mut self, count: u32) -> Result<(), Error> {
            self.account_count = count;
            Ok(())
        }
    }

    impl CodeStore for DummyState {
        fn insert_script(&mut self, script_hash: H256, script: Script) {
            self.scripts.insert(script_hash, script);
        }
        fn get_script(&self, script_hash: &H256) -> Option<Script> {
            self.scripts.get(script_hash).cloned()
        }
        fn get_script_hash_by_short_script_hash(&self, script_hash_prefix: &[u8]) -> Option<H256> {
            self.scripts.iter().find_map(|(script_hash, _script)| {
                let prefix_len = script_hash_prefix.len();
                if &script_hash.as_slice()[..prefix_len] == script_hash_prefix {
                    Some(*script_hash)
                } else {
                    None
                }
            })
        }
        fn insert_data(&mut self, script_hash: H256, code: Bytes) {
            self.codes.insert(script_hash, code);
        }
        fn get_data(&self, script_hash: &H256) -> Option<Bytes> {
            self.codes.get(script_hash).cloned()
        }
    }

    #[test]
    fn test_polyjuice_contract_creator_allowlist() {
        let deployment_script = Script::new_builder()
            .code_hash(TEST_POLYJUICE_SCRIPT_CODE_HASH.pack())
            .hash_type(ScriptHashType::Type.into())
            .args([0u8; 20].to_vec().pack())
            .build();

        let mut dummy_state = DummyState::default();
        while dummy_state.get_account_count().unwrap() < 2 {
            let mut script_hash = [0u8; 32];
            script_hash[0] = dummy_state.get_account_count().unwrap() as u8;
            dummy_state.create_account(script_hash.into()).unwrap();
        }

        let deployment_id = dummy_state
            .create_account(deployment_script.hash().into())
            .unwrap();
        dummy_state.insert_script(deployment_script.hash().into(), deployment_script);

        let allowed_creator_id = {
            let eth_script = Script::new_builder()
                .code_hash(TEST_ETH_SCRIPT_CODE_HASH.pack())
                .hash_type(ScriptHashType::Type.into())
                .args([42u8; 52].pack())
                .build();
            dummy_state.insert_script(eth_script.hash().into(), eth_script.clone());
            dummy_state
                .create_account(eth_script.hash().into())
                .unwrap()
        };

        let allowlist = PolyjuiceContractCreatorAllowList::new(
            TEST_POLYJUICE_SCRIPT_CODE_HASH.into(),
            HashSet::from_iter(vec![[42u8; 20]]),
        );

        // Creator from allowlist should be ok
        let create_contract_tx = RawL2Transaction::new_builder()
            .from_id(allowed_creator_id.pack())
            .to_id(deployment_id.pack())
            .args(Bytes::from(vec![3u8; 10]).pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &create_contract_tx)
            .is_ok());

        // Creator not in allowlist should be error
        let non_allowed_creator_id = {
            let eth_script = Script::new_builder()
                .code_hash(TEST_ETH_SCRIPT_CODE_HASH.pack())
                .hash_type(ScriptHashType::Type.into())
                .args([100u8; 52].pack())
                .build();
            dummy_state.insert_script(eth_script.hash().into(), eth_script.clone());
            dummy_state
                .create_account(eth_script.hash().into())
                .unwrap()
        };
        let create_contract_tx = RawL2Transaction::new_builder()
            .from_id(non_allowed_creator_id.pack())
            .to_id(deployment_id.pack())
            .args(Bytes::from(vec![3u8; 10]).pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &create_contract_tx)
            .is_err());

        // Non contract creation should be ok
        let non_create_contract_tx = RawL2Transaction::new_builder()
            .from_id(non_allowed_creator_id.pack())
            .to_id(deployment_id.pack())
            .args(Bytes::from(vec![0u8; 10]).pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &non_create_contract_tx)
            .is_ok());

        // Not a polyjuice tx should be ok
        let not_polyjuice_script = Script::new_builder()
            .code_hash([11u8; 32].pack())
            .hash_type(ScriptHashType::Type.into())
            .build();
        let not_polyjuice_id = dummy_state
            .create_account(not_polyjuice_script.hash().into())
            .unwrap();
        dummy_state.insert_script(not_polyjuice_script.hash().into(), not_polyjuice_script);
        let not_polyjuice_tx = RawL2Transaction::new_builder()
            .from_id(non_allowed_creator_id.pack())
            .to_id(not_polyjuice_id.pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &not_polyjuice_tx)
            .is_ok());

        // Call meta contract should be failed
        let reserve_script_0_tx = RawL2Transaction::new_builder()
            .from_id(non_allowed_creator_id.pack())
            .to_id(0u32.pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &reserve_script_0_tx)
            .is_err());

        // Reversed CKB SUDT script should be ok
        let reserve_script_1_tx = RawL2Transaction::new_builder()
            .from_id(non_allowed_creator_id.pack())
            .to_id(1u32.pack())
            .build();
        assert!(allowlist
            .validate_with_state(&dummy_state, &reserve_script_1_tx)
            .is_ok());
    }
}
