use crate::Store;
use anyhow::{anyhow, Result};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    smt::{default_store::DefaultStore, Store as SMTStore, H256, SMT},
    state::State,
    CKB_SUDT_SCRIPT_ARGS, CKB_SUDT_SCRIPT_HASH,
};
use gw_config::GenesisConfig;
use gw_generator::{
    backend_manage::{META_CONTRACT_VALIDATOR_CODE_HASH, SUDT_VALIDATOR_CODE_HASH},
    traits::StateExt,
};
use gw_types::{
    core::Status,
    packed::{
        AccountMerkleState, BlockMerkleState, GlobalState, HeaderInfo, L2Block, RawL2Block, Script,
    },
    prelude::*,
};

/// Build genesis block
pub fn build_genesis(config: &GenesisConfig) -> Result<GenesisWithGlobalState> {
    // let mut store: Store<DefaultStore<H256>> = Store::default();
    let mut store: Store = unreachable!();
    build_genesis_from_store(&mut store, config)
}

pub struct GenesisWithGlobalState {
    pub genesis: L2Block,
    pub global_state: GlobalState,
}

fn build_genesis_from_store(
    store: &mut Store,
    config: &GenesisConfig,
) -> Result<GenesisWithGlobalState> {
    unimplemented!()
    // let root = store.calculate_root()?;
    // if !root.is_zero() {
    //     return Err(anyhow!("initial state root must be ZERO"));
    // }

    // // create a reserved account
    // // this account is reserved for special use
    // // for example: send a tx to reserved account to create a new contract account
    // let reserved_id = store.create_account_from_script(
    //     Script::new_builder()
    //         .code_hash({
    //             let code_hash: [u8; 32] = (*META_CONTRACT_VALIDATOR_CODE_HASH).into();
    //             code_hash.pack()
    //         })
    //         .build(),
    // )?;
    // assert_eq!(
    //     reserved_id, RESERVED_ACCOUNT_ID,
    //     "reserved account id must be zero"
    // );

    // // setup CKB simple UDT contract
    // let ckb_sudt_script = Script::new_builder()
    //     .code_hash({
    //         let code_hash: [u8; 32] = (*SUDT_VALIDATOR_CODE_HASH).into();
    //         code_hash.pack()
    //     })
    //     .args(CKB_SUDT_SCRIPT_ARGS.to_vec().pack())
    //     .build();
    // assert_eq!(
    //     ckb_sudt_script.hash(),
    //     CKB_SUDT_SCRIPT_HASH,
    //     "ckb simple UDT script hash"
    // );
    // let ckb_sudt_id = store.create_account_from_script(ckb_sudt_script)?;
    // assert_eq!(
    //     ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
    //     "ckb simple UDT account id"
    // );

    // // calculate post state
    // let post_account = {
    //     let root = store.calculate_root()?;
    //     let count = store.get_account_count()?;
    //     let root: [u8; 32] = root.into();
    //     AccountMerkleState::new_builder()
    //         .merkle_root(root.pack())
    //         .count(count.pack())
    //         .build()
    // };

    // let raw_genesis = RawL2Block::new_builder()
    //     .number(0u64.pack())
    //     .aggregator_id(0u32.pack())
    //     .timestamp(config.timestamp.pack())
    //     .post_account(post_account.clone())
    //     .build();

    // // generate block proof
    // let genesis_hash = raw_genesis.hash();
    // let (block_root, block_proof) = {
    //     let block_key = RawL2Block::compute_smt_key(0);
    //     let mut smt: SMT<DefaultStore<H256>> = Default::default();
    //     smt.update(block_key.into(), genesis_hash.into())?;
    //     let block_proof = smt
    //         .merkle_proof(vec![block_key.into()])?
    //         .compile(vec![(block_key.into(), genesis_hash.into())])?;
    //     let block_root = smt.root().clone();
    //     (block_root, block_proof)
    // };

    // // build genesis
    // let genesis = L2Block::new_builder()
    //     .raw(raw_genesis)
    //     .block_proof(block_proof.0.pack())
    //     .build();
    // let global_state = {
    //     let post_block = BlockMerkleState::new_builder()
    //         .merkle_root({
    //             let root: [u8; 32] = block_root.into();
    //             root.pack()
    //         })
    //         .count(1u64.pack())
    //         .build();
    //     GlobalState::new_builder()
    //         .account(post_account)
    //         .block(post_block)
    //         .status((Status::Running as u8).into())
    //         .build()
    // };
    // store.set_tip_global_state(global_state.clone())?;
    // Ok(GenesisWithGlobalState {
    //     genesis,
    //     global_state,
    // })
}

impl Store {
    pub fn init_genesis(&mut self, config: &GenesisConfig, header: HeaderInfo) -> Result<()> {
        unimplemented!()
        // let GenesisWithGlobalState {
        //     genesis,
        //     global_state: _,
        // } = build_genesis_from_store(self, config)?;
        // self.insert_block(genesis.clone(), header, Vec::new())?;
        // self.attach_block(genesis)?;
        // Ok(())
    }
}
