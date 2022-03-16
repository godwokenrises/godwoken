use gw_common::{
    error::Error,
    smt::{default_store::DefaultStore, H256, SMT},
    state::State,
};
use gw_traits::CodeStore;
use gw_types::{bytes::Bytes, packed::Script};
use std::collections::HashMap;

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
    fn insert_data(&mut self, data_hash: H256, code: Bytes) {
        self.codes.insert(data_hash, code);
    }
    fn get_data(&self, script_hash: &H256) -> Option<Bytes> {
        self.codes.get(script_hash).cloned()
    }
}

#[cfg(test)]
mod tests {
    use ckb_vm::Bytes;
    use gw_common::{
        blake2b::new_blake2b, h256_ext::H256Ext, registry_address::RegistryAddress, state::State,
        H256,
    };
    use gw_traits::CodeStore;
    use gw_types::{
        packed::Script,
        prelude::{Builder, Entity, Pack},
    };

    use crate::{traits::StateExt, Error};

    use super::DummyState;

    #[test]
    fn test_account_with_duplicate_script() {
        let mut tree = DummyState::default();
        let script = Script::new_builder()
            .args([0u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();

        // create duplicate account
        let id = tree.create_account_from_script(script.clone()).unwrap();
        assert_eq!(id, 0);
        let err = tree.create_account_from_script(script.clone()).unwrap_err();
        assert_eq!(
            err,
            Error::State(gw_common::error::Error::DuplicatedScriptHash)
        );

        // create duplicate account
        let err2 = tree.create_account(script.hash().into()).unwrap_err();
        assert_eq!(err2, gw_common::error::Error::DuplicatedScriptHash);
    }

    #[test]
    fn test_query_account() {
        let mut tree = DummyState::default();
        let script_a = Script::new_builder()
            .args([0u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();
        let script_b = Script::new_builder()
            .args([1u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();

        // query account info
        for (expected_id, script) in [script_a, script_b].iter().enumerate() {
            let id = tree.create_account_from_script(script.to_owned()).unwrap();
            assert_eq!(id, expected_id as u32);
            assert_eq!(tree.get_account_count().unwrap(), (expected_id + 1) as u32);
            assert_eq!(
                tree.get_account_id_by_script_hash(&script.hash().into())
                    .unwrap()
                    .unwrap(),
                id
            );
            assert_eq!(tree.get_script_hash(id).unwrap(), script.hash().into());
            assert_eq!(&tree.get_script(&script.hash().into()).unwrap(), script);
        }
    }

    #[test]
    fn test_nonce() {
        let mut tree = DummyState::default();
        let script = Script::new_builder()
            .args([0u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();
        let id = tree.create_account_from_script(script).unwrap();
        assert_eq!(id, 0);
        // query account info
        for i in 1..15 {
            tree.set_nonce(id, i).unwrap();
            assert_eq!(tree.get_nonce(id).unwrap(), i);
        }
    }

    #[test]
    fn test_kv() {
        let mut tree = DummyState::default();
        let script = Script::new_builder()
            .args([0u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();
        let id = tree.create_account_from_script(script).unwrap();
        assert_eq!(id, 0);
        // query account info
        for i in 1..15 {
            let key = H256::from_u32(i as u32);
            let value = H256::from_u32(i as u32);
            tree.update_value(id, key.as_slice(), value).unwrap();
            assert_eq!(tree.get_value(id, key.as_slice()).unwrap(), value);
        }
    }

    #[test]
    fn test_sudt() {
        let mut tree = DummyState::default();
        let script = Script::new_builder()
            .args([0u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();
        let id = tree.create_account_from_script(script).unwrap();
        assert_eq!(id, 0);
        let script = Script::new_builder()
            .args([1u8; 42].pack())
            .hash_type(gw_types::core::ScriptHashType::Type.into())
            .build();
        let sudt_id = tree.create_account_from_script(script).unwrap();
        assert_eq!(sudt_id, 1);
        // mint sudt
        let user_a = RegistryAddress::new(0, vec![1u8; 20]);
        let user_b = RegistryAddress::new(0, vec![2u8; 20]);
        tree.mint_sudt(sudt_id, &user_a, 100).unwrap();
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 100.into());
        tree.mint_sudt(sudt_id, &user_a, 230).unwrap();
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 330.into());
        tree.mint_sudt(sudt_id, &user_b, 155).unwrap();
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 485.into());
        // burn sudt
        tree.burn_sudt(sudt_id, &user_a, 85).unwrap();
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 400.into());
        // overdraft
        let err = tree.burn_sudt(sudt_id, &user_b, 200).unwrap_err();
        assert_eq!(err, gw_common::error::Error::AmountOverflow);
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 400.into());
        tree.burn_sudt(sudt_id, &user_b, 100).unwrap();
        assert_eq!(tree.get_sudt_total_supply(sudt_id).unwrap(), 300.into());
    }

    #[test]
    fn test_data_hash() {
        let mut tree = DummyState::default();
        let data = [42u8; 42];
        let data_hash = {
            let mut hasher = new_blake2b();
            let mut buf = [0u8; 32];
            hasher.update(&data);
            hasher.finalize(&mut buf);
            buf.into()
        };
        tree.insert_data(data_hash, data.to_vec().into());
        // query data
        assert_eq!(
            tree.get_data(&data_hash).unwrap(),
            Bytes::from(data.to_vec())
        );
        // store data hash
        assert!(!tree.is_data_hash_exist(&data_hash).unwrap());
        assert!(!tree.is_data_hash_exist(&H256::zero()).unwrap());
        tree.store_data_hash(data_hash).unwrap();
        assert!(tree.is_data_hash_exist(&data_hash).unwrap());
    }
}
