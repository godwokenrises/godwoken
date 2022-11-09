use ckb_vm::Bytes;
use gw_common::state::State;
use gw_store::{chain_view::ChainView, traits::chain_store::ChainStore};
use gw_types::{
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack},
};

use crate::helper::{
    create_block_producer, deploy, new_block_info, setup, MockContractInfo, PolyjuiceArgsBuilder,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};

const INIT_CODE: &str = include_str!("./evm-contracts/opcodeTxWithMsg.bin");
#[test]
fn gas_price_test() -> anyhow::Result<()> {
    let (store, mut state, generator) = setup();
    let block_producer = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        crate::helper::create_eth_eoa_account(&mut state, &from_eth_address, 200000000u64.into());

    // Deploy Contract
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        INIT_CODE,
        1284600,
        0,
        block_producer.clone(),
        0,
    );

    let contract_account = MockContractInfo::create(&from_eth_address, 0);
    let new_account_id = state
        .get_account_id_by_script_hash(&contract_account.script_hash)?
        .unwrap();

    //call getCurrentGasPrice
    let input = hex::decode("11af6564")?;
    let block_info = new_block_info(block_producer, 1, 0);
    let gas_price = 0x111;
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(228060)
        .gas_price(gas_price)
        .value(10000)
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
        .expect("Call getCurrentGasPrice()");
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&run_result.return_data[16..]);
    let tx_gas_price = u128::from_be_bytes(arr);
    assert_eq!(tx_gas_price as u128, gas_price);
    Ok(())
}
