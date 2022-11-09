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

const INIT_CODE: &str = include_str!("./evm-contracts/AbsentAddress.bin");
#[test]
fn absent_address_test() -> anyhow::Result<()> {
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

    // 0xdB81D2b8154A10C6f25bC2a9225F403D954D0B65 is an unregistered eth_address.
    // call getBalance("0xdB81D2b8154A10C6f25bC2a9225F403D954D0B65")
    let input =
        hex::decode("f8b2cb4f000000000000000000000000db81d2b8154a10c6f25bc2a9225f403d954d0b65")?;
    let block_info = new_block_info(block_producer.clone(), 1, 0);
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(228060)
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
        .expect("Call getBalance");
    assert_eq!(run_result.return_data.as_ref(), &[0u8; 32]);

    //call getCodeSize("0xdB81D2b8154A10C6f25bC2a9225F403D954D0B65")
    let input =
        hex::decode("b51c4f96000000000000000000000000db81d2b8154a10c6f25bc2a9225f403d954d0b65")?;
    let block_info = new_block_info(block_producer.clone(), 1, 0);
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(228060)
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
        .expect("Call getCodeSize");
    assert_eq!(run_result.return_data.as_ref(), &[0u8; 32]);

    //call getCodeHash("0xdB81D2b8154A10C6f25bC2a9225F403D954D0B65")
    let input =
        hex::decode("81ea4408000000000000000000000000db81d2b8154a10c6f25bc2a9225f403d954d0b65")?;
    let block_info = new_block_info(block_producer.clone(), 1, 0);
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(228060)
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
        .expect("Call getCodeHash");
    assert_eq!(run_result.return_data.as_ref(), &[0u8; 32]);

    //call getCode("0xdB81D2b8154A10C6f25bC2a9225F403D954D0B65")
    let input =
        hex::decode("7e105ce2000000000000000000000000db81d2b8154a10c6f25bc2a9225f403d954d0b65")?;
    let block_info = new_block_info(block_producer, 1, 0);
    let args = PolyjuiceArgsBuilder::default()
        .gas_limit(228060)
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
        .expect("Call getCode");
    let mut target = [0u8; 64];
    target[31] = 32;
    assert_eq!(run_result.return_data.as_ref(), &target);
    Ok(())
}
