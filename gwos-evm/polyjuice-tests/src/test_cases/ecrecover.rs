use crate::helper::{
    self, deploy, new_block_info, setup, MockContractInfo, PolyjuiceArgsBuilder,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::state::State;
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const INIT_CODE: &str = include_str!("./evm-contracts/HeadTail.bin");

#[test]
fn test_ecrecover() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 500000u64.into());

    // Deploy HeadTail Contract
    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        INIT_CODE,
        187881,
        0,
        block_producer_id.clone(),
        1,
    );
    // [Deploy HeadTail Contract] used cycles: 1645593 < 1650K
    helper::check_cycles("Deploy HeadTail Contract", run_result.cycles, 2_000_000);
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );

    let contract_account = MockContractInfo::create(&from_eth_address, 0);
    let contract_account_script_hash = contract_account.script_hash;
    let new_account_id = state
        .get_account_id_by_script_hash(&contract_account_script_hash)
        .unwrap()
        .unwrap();

    {
        // verify|recover(bytes32 hash, bytes memory signature)
        let block_info = new_block_info(block_producer_id, 2, 0);
        let hash = "8ab0890f028c9502cc20d441b4c4bb116f48ea632f522ac84e965d1dadf918e1";
        let signed_hash = "aaa99f644a5c4447314c5b7fcfac80deb186218aca1edaa63711aa75eb36585b47743901ce20f32768c7108bf85457ee0f16020f9bebc2bf456d6094c1c923c11c";
        let input = hex::decode(format!(
            "258ae582{}00000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000041{}00000000000000000000000000000000000000000000000000000000000000",
            hash,
            signed_hash,
        )).unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(50000)
            .gas_price(1)
            .value(0)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(new_account_id.pack())
            .args(Bytes::from(args).pack())
            .build();
        let db = &store.begin_transaction();
        let tip_block_hash = db.get_tip_block_hash().unwrap();
        let run_result = generator
            .execute_transaction(
                &ChainView::new(&db, tip_block_hash),
                &mut state,
                &block_info,
                &raw_tx,
                L2TX_MAX_CYCLES,
                None,
            )
            .expect("construct");
        // [recover] used cycles: 2604412 < 2660K
        helper::check_cycles("verify|recover", run_result.cycles, 2_960_000);
        state.finalise().expect("update state");
        assert_eq!(
            run_result.return_data,
            hex::decode("000000000000000000000000f175db82ceaaadd50a606d70e389e9a1284a6690")
                .unwrap()
        );
        println!("return_data: {}", hex::encode(&run_result.return_data));
    }
}
