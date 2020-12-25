use std::collections::HashMap;

use crate::Store;
use anyhow::Result;
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    smt::{default_store::DefaultStore, H256, SMT},
    sparse_merkle_tree::tree::{BranchNode, LeafNode},
    state::State,
    CKB_SUDT_SCRIPT_HASH, SUDT_CODE_HASH,
};
use gw_config::GenesisConfig;
use gw_generator::traits::StateExt;
use gw_types::{
    core::Status,
    packed::{AccountMerkleState, BlockMerkleState, GlobalState, L2Block, RawL2Block, Script},
    prelude::*,
};

pub struct GenesisWithSMTState {
    pub genesis: L2Block,
    pub global_state: GlobalState,
    pub branches_map: HashMap<H256, BranchNode>,
    pub leaves_map: HashMap<H256, LeafNode<H256>>,
}

pub fn build_genesis(config: &GenesisConfig) -> Result<GenesisWithSMTState> {
    // build initialized states
    let mut state: Store<DefaultStore<H256>> = Default::default();
    let root = state.calculate_root()?;
    assert!(root.is_zero(), "initial root must be ZERO");

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_id = state.create_account_from_script(
        Script::new_builder()
            .code_hash([0u8; 32].pack())
            .args([0u8; 20].to_vec().pack())
            .build(),
    )?;
    assert_eq!(
        reserved_id, RESERVED_ACCOUNT_ID,
        "reserved account id must be zero"
    );

    // setup CKB simple UDT contract
    let ckb_sudt_script = Script::new_builder()
        .code_hash(SUDT_CODE_HASH.pack())
        .args([0u8; 32].to_vec().pack())
        .build();
    assert_eq!(
        ckb_sudt_script.hash(),
        CKB_SUDT_SCRIPT_HASH,
        "ckb simple UDT script hash"
    );
    let ckb_sudt_id = state.create_account_from_script(ckb_sudt_script)?;
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );

    // calculate post state
    let post_account = {
        let root = state.calculate_root()?;
        let count = state.get_account_count()?;
        let root: [u8; 32] = root.into();
        AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build()
    };

    let raw_genesis = RawL2Block::new_builder()
        .number(0u64.pack())
        .aggregator_id(0u32.pack())
        .timestamp(config.timestamp.pack())
        .post_account(post_account.clone())
        .build();

    // generate block proof
    let genesis_hash = raw_genesis.hash();
    let (block_root, block_proof) = {
        let block_key = RawL2Block::compute_smt_key(0);
        let mut smt: SMT<DefaultStore<H256>> = Default::default();
        smt.update(block_key.into(), genesis_hash.into())?;
        let block_proof = smt
            .merkle_proof(vec![block_key.into()])?
            .compile(vec![(block_key.into(), genesis_hash.into())])?;
        let block_root = smt.root().clone();
        (block_root, block_proof)
    };

    // build genesis
    let genesis = L2Block::new_builder()
        .raw(raw_genesis)
        .block_proof(block_proof.0.pack())
        .build();
    let global_state = {
        let post_block = BlockMerkleState::new_builder()
            .merkle_root({
                let root: [u8; 32] = block_root.into();
                root.pack()
            })
            .count(1u64.pack())
            .build();
        GlobalState::new_builder()
            .account(post_account)
            .block(post_block)
            .status((Status::Running as u8).into())
            .build()
    };
    let store = state.account_smt().store();
    {
        let inner = store.inner().lock();
        Ok(GenesisWithSMTState {
            genesis,
            global_state,
            leaves_map: inner.leaves_map().clone(),
            branches_map: inner.branches_map().clone(),
        })
    }
}
