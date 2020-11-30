use anyhow::{anyhow, Result};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, RESERVED_ACCOUNT_ID},
    smt::{default_store::DefaultStore, H256, SMT},
    state::State,
    CKB_TOKEN_ID, SUDT_CODE_HASH,
};
use gw_config::GenesisConfig;
use gw_generator::traits::StateExt;
use gw_store::Store;
use gw_types::{
    packed::{AccountMerkleState, L2Block, RawL2Block, Script},
    prelude::*,
};

pub fn build_genesis(config: &GenesisConfig) -> Result<L2Block> {
    // build initialized states
    let mut state: Store<DefaultStore<H256>> = Default::default();
    let root = state
        .calculate_root()
        .map_err(|err| anyhow!("calculate root error: {:?}", err))?;
    assert!(root.is_zero(), "initial root must be ZERO");

    // create a reserved account
    // this account is reserved for special use
    // for example: send a tx to reserved account to create a new contract account
    let reserved_id = state
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .map_err(|err| anyhow!("create reserved account error: {:?}", err))?;
    assert_eq!(
        reserved_id, RESERVED_ACCOUNT_ID,
        "reserved account id must be zero"
    );

    // setup CKB simple UDT contract
    let ckb_sudt_id = state
        .create_account_from_script(
            Script::new_builder()
                .code_hash(SUDT_CODE_HASH.pack())
                .args(CKB_TOKEN_ID.to_vec().pack())
                .build(),
        )
        .map_err(|err| anyhow!("create reserved account error: {:?}", err))?;
    assert_eq!(
        ckb_sudt_id, CKB_SUDT_ACCOUNT_ID,
        "ckb simple UDT account id"
    );

    // create initial aggregator
    let initial_aggregator_id = {
        let pubkey_hash: [u8; 20] = config.initial_aggregator_pubkey_hash.clone().into();
        state
            .create_account_from_script(
                Script::new_builder()
                    .code_hash([0u8; 32].pack())
                    .args(pubkey_hash.to_vec().pack())
                    .build(),
            )
            .map_err(|err| anyhow!("create initial aggregator error: {:?}", err))?
    };
    state
        .mint_sudt(
            ckb_sudt_id,
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
        .post_account(post_account)
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
