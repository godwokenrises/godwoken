use crate::helper::{self, MockContractInfo, L2TX_MAX_CYCLES};
use gw_common::{
    builtins::{CKB_SUDT_ACCOUNT_ID, ETH_REGISTRY_ACCOUNT_ID},
    registry_address::RegistryAddress,
    state::State,
};
use gw_store::{chain_view::ChainView, state::traits::JournalDB, traits::chain_store::ChainStore};
use gw_types::{
    bytes::Bytes,
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack},
    U256,
};

const CONTRACT_CODE: &str = include_str!("./evm-contracts/BeaconProxy.bin");

#[test]
fn test_beacon_proxy() {
    let (store, mut state, generator) = helper::setup();
    let block_producer = helper::create_block_producer(&mut state);
    let mut block_number = 0;

    // init accounts
    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, U256::from(2000000u64));

    // deploy payableInitializationBugContractTest contract
    let run_result = helper::deploy(
        &generator,
        &store,
        &mut state,
        helper::CREATOR_ACCOUNT_ID,
        from_id,
        CONTRACT_CODE,
        233000,
        0,
        block_producer.to_owned(),
        block_number,
    );
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);
    let contract = MockContractInfo::create(&from_eth_address, 0);
    let contract_account_id = state
        .get_account_id_by_script_hash(&contract.script_hash)
        .unwrap()
        .expect("get_account_id_by_script_hash");

    // invoke payableInitializationBugContractTest.init() -> 0xe1c7392a
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input = hex::decode("e1c7392a").expect("init() method ID");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(1829630)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("invode initializaion");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);
    state.finalise().expect("update state");

    state
        .mint_sudt(CKB_SUDT_ACCOUNT_ID, &contract.reg_addr, U256::from(200u64))
        .unwrap();

    // deployBeaconProxy(
    //   upgradeable_beacon_addr,
    //   ethers.utils.arrayify("0xe79f5bee0000000000000000000000000000000000000000000000000000000000000037"),
    //   { value: 110 })
    block_number += 1;
    let block_info = helper::new_block_info(block_producer.to_owned(), block_number, block_number);
    let input = hex::decode("dc3bdc5500000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000024e79f5bee000000000000000000000000000000000000000000000000000000000000003700000000000000000000000000000000000000000000000000000000").expect("deployBeaconProxy(bytes)");
    const DEPOLOY_MES_VALUE: u128 = 17;
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(447270)
        .gas_price(1)
        .value(DEPOLOY_MES_VALUE)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("Call deployBeaconProxy");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);
    state.finalise().expect("update state");

    // get BeaconProxy public bpx and check it's balance
    block_number += 1;
    let block_info = helper::new_block_info(block_producer, block_number, block_number);
    let input = hex::decode("c6662850").expect("bpx.get() method id");
    let args = helper::PolyjuiceArgsBuilder::default()
        .gas_limit(23778)
        .gas_price(1)
        .value(0)
        .input(&input)
        .build();
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(contract_account_id.pack())
        .args(Bytes::from(args).pack())
        .build();
    let db = &store.begin_transaction();
    let tip_block_hash = store.get_tip_block_hash().unwrap();
    let run_result = generator
        .execute_transaction(
            &ChainView::new(&db, tip_block_hash),
            &mut state,
            &block_info,
            &raw_tx,
            L2TX_MAX_CYCLES,
            None,
        )
        .expect("get BeaconProxy contract address");
    assert_eq!(run_result.exit_code, crate::constant::EVMC_SUCCESS);
    state.finalise().expect("update state");
    assert_eq!(run_result.return_data.len(), 32);
    let beacon_proxy_ethabi_addr = &run_result.return_data[12..];
    let beacon_reg_addr =
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, beacon_proxy_ethabi_addr.to_vec());
    assert_eq!(
        state
            .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &beacon_reg_addr)
            .unwrap(),
        U256::from(DEPOLOY_MES_VALUE),
        "check the balance of BeaconProxy contract"
    );
}
