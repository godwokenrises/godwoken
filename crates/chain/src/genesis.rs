use crate::state_impl::StateImpl;
use anyhow::{anyhow, Result};
use gw_common::{
    smt::{default_store::DefaultStore, H256, SMT},
    state::{State, ZERO},
    CKB_TOKEN_ID,
};
use gw_config::GenesisConfig;
use gw_types::{
    packed::{AccountMerkleState, L2Block, RawL2Block},
    prelude::*,
};

pub fn build_genesis(config: &GenesisConfig) -> Result<L2Block> {
    // build initialized states
    let mut state: StateImpl<DefaultStore<H256>> = Default::default();
    let root = state
        .calculate_root()
        .map_err(|err| anyhow!("calculate root error: {:?}", err))?;
    assert_eq!(root, ZERO, "initial root must be ZERO");

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_account_id = state
        .create_account(ZERO, [0u8; 20])
        .map_err(|err| anyhow!("create reserved account error: {:?}", err))?;
    assert_eq!(reserved_account_id, 0, "reserved account id must be zero");

    // TODO setup the simple UDT contract

    // create initial aggregator
    let initial_aggregator_id = {
        let pubkey_hash = config.initial_aggregator_pubkey_hash.clone().into();
        state
            .create_account(ZERO, pubkey_hash)
            .map_err(|err| anyhow!("create initial aggregator error: {:?}", err))?
    };
    state
        .mint_sudt(
            &CKB_TOKEN_ID,
            initial_aggregator_id,
            config.initial_deposition.into(),
        )
        .map_err(|err| anyhow!("mint sudt error: {:?}", err))?;

    // calculate post state
    let post_account = {
        let root = state
            .calculate_root()
            .map_err(|err| anyhow!("calculate root error: {:?}", err))?;
        let count = state
            .get_account_count()
            .map_err(|err| anyhow!("get account count error: {:?}", err))?;
        AccountMerkleState::new_builder()
            .merkle_root(root.pack())
            .count(count.pack())
            .build()
    };

    let raw_genesis = RawL2Block::new_builder()
        .number(0u64.pack())
        .aggregator_id(0u32.pack())
        .timestamp(config.timestamp.pack())
        .post_account(post_account)
        .valid(1.into())
        .build();

    // generate block proof
    let genesis_hash = raw_genesis.hash();
    let block_proof = {
        let block_key = RawL2Block::compute_smt_key(0);
        let mut smt: SMT<DefaultStore<H256>> = Default::default();
        smt.update(block_key.into(), genesis_hash.into())
            .map_err(|err| anyhow!("update smt error: {:?}", err))?;
        smt.merkle_proof(vec![block_key.into()])
            .map_err(|err| anyhow!("gen merkle proof error: {:?}", err))?
            .compile(vec![(block_key.into(), genesis_hash.into())])
            .map_err(|err| anyhow!("compile merkle proof error: {:?}", err))?
    };

    // build genesis
    let genesis = L2Block::new_builder()
        .raw(raw_genesis)
        .block_proof(block_proof.0.pack())
        .build();
    Ok(genesis)
}
