//! Test contract call contract
//!   See ./evm-contracts/CallContract.sol

use crate::helper::{
    self, compute_create2_script, contract_script_to_eth_addr, deploy, new_block_info, parse_log,
    setup, simple_storage_get, Log, MockContractInfo, PolyjuiceArgsBuilder, CKB_SUDT_ACCOUNT_ID,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};
use gw_common::{builtins::ETH_REGISTRY_ACCOUNT_ID, state::State};
use gw_store::traits::chain_store::ChainStore;
use gw_store::{chain_view::ChainView, state::traits::JournalDB};
use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*, U256};

const SS_INIT_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");
const CREATE2_IMPL_CODE: &str = include_str!("./evm-contracts/Create2Impl.bin");

#[test]
fn test_create2() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 2000000u64.into());

    // Deploy CREATE2_IMPL_CODE
    let mut block_number = 1;
    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        CREATE2_IMPL_CODE,
        122000,
        0,
        block_producer_id.clone(),
        block_number,
    );
    // [Deploy Create2Impl] used cycles: 819215 < 820K
    helper::check_cycles("Deploy Create2Impl", run_result.cycles, 1_200_000);
    // println!(
    //     "result {}",
    //     serde_json::to_string_pretty(&RunResult::from(run_result)).unwrap()
    // );
    let create2_contract = MockContractInfo::create(&from_eth_address, 0);
    let create2_contract_addr = create2_contract.eth_addr;
    let create2_contract_script_hash = create2_contract.script_hash;
    let create2_contract_id = state
        .get_account_id_by_script_hash(&create2_contract_script_hash)
        .unwrap()
        .unwrap();
    println!("create2_contract account id = {}", create2_contract_id);
    let address = state
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &create2_contract_script_hash)
        .unwrap()
        .unwrap();
    let create2_contract_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &address)
        .unwrap();
    assert_eq!(create2_contract_balance, U256::zero());

    let input_value_u128: u128 = 0x9a;
    // bytes32 salt
    let input_salt = "1111111111111111111111111111111111111111111111111111111111111111";

    // Create2Impl.deploy(uint256 value, bytes32 salt, bytes memory code)
    let run_result = {
        block_number += 1;
        let block_info = new_block_info(block_producer_id, block_number, block_number);
        //consturct input:
        //0x92
        //input_salt
        //SS_INIT_CODE
        let input = hex::decode("66cfa05700000000000000000000000000000000000000000000000000000000000000921111111111111111111111111111111111111111111111111111111111111111000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000ea6080604052607b60008190555060d08061001a6000396000f3fe60806040526004361060295760003560e01c806360fe47b11460345780636d4ce63c14605f57602f565b36602f57005b600080fd5b605d60048036036020811015604857600080fd5b81019080803590602001909291905050506087565b005b348015606a57600080fd5b5060716091565b6040518082815260200191505060405180910390f35b8060008190555050565b6000805490509056fea2646970667358221220b796688cdcda21059332f8ef75088337063fcf7a8ab96bb23bc06ec8623d679064736f6c6343000602003300000000000000000000000000000000000000000000").unwrap();

        let args = PolyjuiceArgsBuilder::default()
            .gas_limit(91000)
            .gas_price(1)
            .value(input_value_u128)
            .input(&input)
            .build();
        let raw_tx = RawL2Transaction::new_builder()
            .from_id(from_id.pack())
            .to_id(create2_contract_id.pack())
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
            .expect("Create2Impl.deploy(uint256 value, bytes32 salt, bytes memory code)");
        // [Create2Impl.deploy(...)] used cycles: 1197555 < 1230K
        helper::check_cycles(
            "Create2Impl.deploy(...)",
            run_result.cycles.to_owned(),
            1_750_000,
        );
        state.finalise().expect("update state");
        run_result
    };
    // [Create2Impl.deploy(...)] used cycles: 1197555 < 1230K
    helper::check_cycles("Create2Impl.deploy(...)", run_result.cycles, 1_750_000);

    let create2_script = compute_create2_script(
        create2_contract_addr.as_slice(),
        &hex::decode(input_salt).unwrap()[..],
        &hex::decode(SS_INIT_CODE).unwrap()[..],
    );
    let create2_script_hash = create2_script.hash();
    let create2_ethabi_addr = contract_script_to_eth_addr(&create2_script, true);
    println!(
        "computed create2_ethabi_addr: {}",
        hex::encode(&create2_ethabi_addr)
    );
    println!(
        "create2_address: 0x{}",
        hex::encode(&run_result.return_data)
    );
    assert_eq!(run_result.return_data, create2_ethabi_addr);
    let create2_account_id = state
        .get_account_id_by_script_hash(&create2_script_hash.into())
        .unwrap()
        .unwrap();
    let address = state
        .get_registry_address_by_script_hash(ETH_REGISTRY_ACCOUNT_ID, &create2_script_hash.into())
        .unwrap()
        .unwrap();
    let create2_account_balance = state
        .get_sudt_balance(CKB_SUDT_ACCOUNT_ID, &address)
        .unwrap();

    let log = parse_log(&run_result.logs[1]);

    let balance = match log {
        Log::SudtTransfer {
            sudt_id: _,
            from_addr,
            to_addr,
            amount,
        } => {
            println!(
                "transfer from: {}, to: {}, amount: {}",
                hex::encode(&from_addr.address),
                hex::encode(&to_addr.address),
                amount
            );
            Some(amount)
        }
        _ => None,
    };
    assert_eq!(Some(create2_account_balance), balance);

    let run_result = simple_storage_get(
        &store,
        &mut state,
        &generator,
        block_number,
        from_id,
        create2_account_id,
    );
    assert_eq!(
        run_result.return_data,
        hex::decode("000000000000000000000000000000000000000000000000000000000000007b").unwrap()
    );
}
