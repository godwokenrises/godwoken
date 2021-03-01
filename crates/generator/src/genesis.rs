use crate::{builtin_scripts::META_CONTRACT_VALIDATOR_CODE_HASH, traits::StateExt};
use anyhow::Result;
use gw_common::{
    blake2b::new_blake2b,
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    smt::{default_store::DefaultStore, H256, SMT},
    state::State,
    CKB_SUDT_SCRIPT_ARGS,
};
use gw_config::GenesisConfig;
use gw_store::{
    state_db::{StateDBTransaction, StateDBVersion},
    transaction::StoreTransaction,
    Store,
};
use gw_types::{
    core::Status,
    packed::{
        AccountMerkleState, BlockMerkleState, GlobalState, HeaderInfo, L2Block, RawL2Block,
        RollupConfig, Script,
    },
    prelude::*,
};

/// Build genesis block
pub fn build_genesis(
    config: &GenesisConfig,
    rollup_config: &RollupConfig,
) -> Result<GenesisWithGlobalState> {
    let store = Store::open_tmp()?;
    let mut db = store.begin_transaction();
    build_genesis_from_store(&mut db, config, rollup_config)
}

pub struct GenesisWithGlobalState {
    pub genesis: L2Block,
    pub global_state: GlobalState,
}

/// build genesis from store
/// This function initialize db to genesis state
pub fn build_genesis_from_store(
    db: &StoreTransaction,
    config: &GenesisConfig,
    rollup_config: &RollupConfig,
) -> Result<GenesisWithGlobalState> {
    // initialize store
    db.set_account_smt_root(H256::zero())?;
    db.set_block_smt_root(H256::zero())?;
    db.set_account_count(0)?;
    let state_db = StateDBTransaction::from_version(db.clone(), StateDBVersion::from_genesis());
    let mut tree = state_db.account_state_tree()?;

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_id = tree.create_account_from_script(
        Script::new_builder()
            .code_hash({
                let code_hash: [u8; 32] = (*META_CONTRACT_VALIDATOR_CODE_HASH).into();
                code_hash.pack()
            })
            .build(),
    )?;
    assert_eq!(
        reserved_id, RESERVED_ACCOUNT_ID,
        "reserved account id must be zero"
    );

    // setup CKB simple UDT contract
    let ckb_sudt_script = crate::sudt::build_l2_sudt_script(CKB_SUDT_SCRIPT_ARGS.into());
    let ckb_sudt_id = tree.create_account_from_script(ckb_sudt_script)?;
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );

    // calculate post state
    let post_account = {
        let root = tree.calculate_root()?;
        let count = tree.get_account_count()?;
        let root: [u8; 32] = root.into();
        AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build()
    };

    let raw_genesis = RawL2Block::new_builder()
        .number(0u64.pack())
        .block_producer_id(0u32.pack())
        .parent_block_hash([0u8; 32].pack())
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
        let rollup_config_hash = {
            let mut hasher = new_blake2b();
            hasher.update(rollup_config.as_slice());
            let mut hash = [0u8; 32];
            hasher.finalize(&mut hash);
            hash
        };
        GlobalState::new_builder()
            .account(post_account)
            .block(post_block)
            .status((Status::Running as u8).into())
            .rollup_config_hash(rollup_config_hash.pack())
            .tip_block_hash(genesis.hash().pack())
            .build()
    };
    db.set_block_smt_root(global_state.block().merkle_root().unpack())?;
    tree.submit_tree()?;
    Ok(GenesisWithGlobalState {
        genesis,
        global_state,
    })
}

pub fn init_genesis(
    store: &Store,
    config: &GenesisConfig,
    rollup_config: &RollupConfig,
    header: HeaderInfo,
    chain_id: H256,
) -> Result<()> {
    if store.has_genesis()? {
        panic!("The store is already initialized!");
    }
    let mut db = store.begin_transaction();
    db.setup_chain_id(chain_id)?;
    let GenesisWithGlobalState {
        genesis,
        global_state,
    } = build_genesis_from_store(&mut db, config, rollup_config)?;
    db.insert_block(
        genesis.clone(),
        header,
        global_state,
        Vec::new(),
        Vec::new(),
    )?;
    db.attach_block(genesis)?;
    db.commit()?;
    Ok(())
}
