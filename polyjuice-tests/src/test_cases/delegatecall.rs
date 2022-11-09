//! Test contract call contract multiple times
//!   See ./evm-contracts/CallContract.sol

use crate::helper::{
    self, contract_script_to_eth_addr, deploy, new_block_info,
    new_contract_account_script_with_nonce, setup, simple_storage_get, MockContractInfo,
    PolyjuiceArgsBuilder, CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::{builtins::CKB_SUDT_ACCOUNT_ID, state::State};
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

const SS_INIT_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");
const INIT_CODE: &str = include_str!("./evm-contracts/DelegateCall.bin");

#[test]
fn test_delegatecall() {
    let (store, mut state, generator) = setup();
    let block_producer = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 500000u64.into());

    // Deploy SimpleStorage
    let mut block_number = 1;
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        SS_INIT_CODE,
        120000,
        0,
        block_producer.clone(),
        block_number,
    );
    let ss_account_script = new_contract_account_script_with_nonce(&from_eth_address, 0);
    let ss_account_id = state
        .get_account_id_by_script_hash(&ss_account_script.hash().into())
        .unwrap()
        .unwrap();

    // Deploy DelegateCall
    block_number += 1;
    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        INIT_CODE,
        132000,
        0,
        block_producer.clone(),
        block_number,
    );
    // [Deploy DelegateCall] used cycles: 753698 < 760K
    helper::check_cycles("Deploy DelegateCall", run_result.cycles, 1_100_000);
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );
    let delegate_contract = MockContractInfo::create(&from_eth_address, 1);
    let delegate_contract_script_hash = delegate_contract.script_hash;
    let delegate_contract_id = state
        .get_account_id_by_script_hash(&delegate_contract_script_hash)
        .unwrap()
        .unwrap();

    assert_eq!(state.get_nonce(from_id).unwrap(), 2);
    assert_eq!(state.get_nonce(ss_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(delegate_contract_id).unwrap(), 0);

    /*
     * In a delegatecall, only the code of the given address is used, all other aspects (storage,
     * balance, …) are taken from the current contract.
     * The purpose of delegatecall is to use library code which is stored in another contract.
     * The user has to ensure that the layout of storage in both contracts is suitable for
     * delegatecall to be used.
     */
    const MSG_VALUE: u128 = 17;
    for (fn_sighash, expected_return_value) in [
        // DelegateCall.set(address, uint) => used cycles: 1002251
        (
            "3825d828",
            "0000000000000000000000000000000000000000000000000000000000000022",
        ),
        // DelegateCall.overwrite(address, uint) => used cycles: 1002099
        (
            "3144564b",
            "0000000000000000000000000000000000000000000000000000000000000023",
        ),
        // DelegateCall.multiCall(address, uint) => used cycles: 1422033
        (
            "c6c211e9",
            "0000000000000000000000000000000000000000000000000000000000000024",
        ),
    ]
    .iter()
    {
        block_number += 1;
        let block_info = new_block_info(block_producer.clone(), block_number, block_number);
        let input = hex::decode(format!(
            "{}{}{}",
            fn_sighash,
            hex::encode(contract_script_to_eth_addr(&ss_account_script, true)),
            "0000000000000000000000000000000000000000000000000000000000000022",
        ))
        .unwrap();
        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(200000)
            .gas_price(1)
            .value(MSG_VALUE)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(delegate_contract_id.pack())
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
        // [DelegateCall] used cycles: 1457344 < 1460K
        helper::check_cycles("DelegateCall", run_result.cycles, 1_710_000);
        state.finalise().expect("update state");
        // println!(
        //     "result {}",
        //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
        // );

        let run_result = simple_storage_get(
            &store,
            &mut state,
            &generator,
            block_number,
            from_id,
            delegate_contract_id,
        );
        assert_eq!(
            run_result.return_data,
            hex::decode(expected_return_value).unwrap()
        );
    }
    // check the balance of DelegateCall contract
    let delegate_contract_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &delegate_contract.reg_addr)
        .unwrap();
    assert_eq!(
        delegate_contract_balance,
        gw_types::U256::from(MSG_VALUE * 3)
    );
    assert_eq!(state.get_nonce(ss_account_id).unwrap(), 0);
    assert_eq!(state.get_nonce(delegate_contract_id).unwrap(), 0);

    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        ss_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap(),
        "The storedData in SimepleStorage contract won't be changed."
    );
}
