use crate::testing_tool::{
    chain::META_VALIDATOR_SCRIPT_TYPE_HASH, programs::ETH_ADDR_REG_CONTRACT_CODE_HASH,
};
use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    registry_address::RegistryAddress,
    state::State,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_generator::{dummy_state::DummyState, traits::StateExt};
use gw_types::{
    core::ScriptHashType,
    offchain::RollupContext,
    packed::{RollupConfig, Script},
    prelude::*,
};

pub struct TestingContext {
    pub state: DummyState,
    pub eth_registry_id: u32,
}

impl TestingContext {
    pub fn setup(rollup_config: &RollupConfig) -> Self {
        let rollup_context = RollupContext {
            rollup_config: rollup_config.clone(),
            rollup_script_hash: [42u8; 32].into(),
        };

        // deploy registry contract
        let mut state = DummyState::default();

        // setup meta_contract
        let meta_contract_id = state
            .create_account_from_script(
                Script::new_builder()
                    .code_hash(META_VALIDATOR_SCRIPT_TYPE_HASH.clone().pack())
                    .args([0u8; 32].to_vec().pack())
                    .hash_type(ScriptHashType::Type.into())
                    .build(),
            )
            .expect("create account");
        assert_eq!(meta_contract_id, RESERVED_ACCOUNT_ID);

        // setup CKB simple UDT contract
        let ckb_sudt_script =
            gw_generator::sudt::build_l2_sudt_script(&rollup_context, &CKB_SUDT_SCRIPT_ARGS.into());
        let ckb_sudt_id = state.create_account_from_script(ckb_sudt_script).unwrap();
        assert_eq!(
            ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
            "ckb simple UDT account id"
        );

        // setup eth registry id
        let eth_registry_id = state
            .create_account_from_script(
                Script::new_builder()
                    .code_hash(ETH_ADDR_REG_CONTRACT_CODE_HASH.clone().pack())
                    .args(Default::default())
                    .hash_type(ScriptHashType::Type.into())
                    .build(),
            )
            .expect("create registry account");

        Self {
            state,
            eth_registry_id,
        }
    }

    pub fn create_eth_address(
        &mut self,
        script_hash: H256,
        eth_address: [u8; 20],
    ) -> RegistryAddress {
        let registry_address = RegistryAddress::new(self.eth_registry_id, eth_address.to_vec());
        self.state
            .mapping_registry_address_to_script_hash(registry_address.clone(), script_hash)
            .expect("mapping address");
        registry_address
    }
}
