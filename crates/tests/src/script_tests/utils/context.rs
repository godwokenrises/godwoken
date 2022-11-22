use crate::script_tests::programs::ETH_ADDR_REG_CONTRACT_CODE_HASH;
use crate::testing_tool::chain::{
    ALWAYS_SUCCESS_CODE_HASH, ETH_REGISTRY_SCRIPT_TYPE_HASH, META_VALIDATOR_SCRIPT_TYPE_HASH,
    SUDT_VALIDATOR_SCRIPT_TYPE_HASH,
};
use ckb_types::prelude::{Builder, Entity};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    registry_address::RegistryAddress,
    smt::SMT,
    state::State,
    CKB_SUDT_SCRIPT_ARGS, H256,
};
use gw_generator::traits::StateExt;
use gw_store::{
    smt::smt_store::SMTStateStore,
    snapshot::StoreSnapshot,
    state::{
        overlay::{mem_state::MemStateTree, mem_store::MemStore},
        MemStateDB,
    },
    Store,
};
use gw_types::core::AllowedContractType;
use gw_types::packed::AllowedTypeHash;
use gw_types::{
    core::ScriptHashType,
    packed::{RollupConfig, Script},
    prelude::*,
};
use gw_utils::RollupContext;

fn new_state(store: StoreSnapshot) -> MemStateDB {
    let smt = SMT::new(H256::zero(), SMTStateStore::new(MemStore::new(store)));
    let inner = MemStateTree::new(smt, 0);
    MemStateDB::new(inner)
}

pub struct TestingContext {
    pub store: Store,
    pub state: MemStateDB,
    pub eth_registry_id: u32,
    pub rollup_config: RollupConfig,
}

impl TestingContext {
    pub fn default_rollup_config() -> RollupConfig {
        RollupConfig::new_builder()
            .allowed_eoa_type_hashes(
                vec![AllowedTypeHash::from_unknown(*ALWAYS_SUCCESS_CODE_HASH)].pack(),
            )
            .allowed_contract_type_hashes(
                vec![
                    AllowedTypeHash::new_builder()
                        .hash(META_VALIDATOR_SCRIPT_TYPE_HASH.pack())
                        .type_(AllowedContractType::Meta.into())
                        .build(),
                    AllowedTypeHash::new_builder()
                        .hash(SUDT_VALIDATOR_SCRIPT_TYPE_HASH.pack())
                        .type_(AllowedContractType::Sudt.into())
                        .build(),
                    AllowedTypeHash::new_builder()
                        .hash(ETH_REGISTRY_SCRIPT_TYPE_HASH.pack())
                        .type_(AllowedContractType::EthAddrReg.into())
                        .build(),
                ]
                .pack(),
            )
            .build()
    }

    pub fn setup() -> Self {
        Self::setup_with_config(Self::default_rollup_config())
    }
    pub fn setup_with_config(rollup_config: RollupConfig) -> Self {
        let rollup_context = RollupContext {
            rollup_config: rollup_config.clone(),
            rollup_script_hash: [42u8; 32].into(),
            ..Default::default()
        };

        // deploy registry contract
        let store = Store::open_tmp().unwrap();
        let mut state = new_state(store.get_snapshot());

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
            store,
            state,
            eth_registry_id,
            rollup_config,
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
