//! Deploy muti-sign-wallet Contract
//!   See https://github.com/Flouse/godwoken-examples/blob/contracts/packages/polyjuice/contracts/WalletSimple.sol/WalletSimple.json

use crate::helper::{self, new_contract_account_script, CREATOR_ACCOUNT_ID};
use gw_common::state::State;

const BIN_CODE: &str = include_str!("./evm-contracts/SimpleWallet.bin");

#[test]
fn test_simple_wallet() {
    let (store, mut state, generator) = helper::setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 20000000u64.into());

    // Deploy SimpleWallet Contract
    let block_number = 1;
    let run_result = helper::deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        BIN_CODE,
        252898,
        0,
        block_producer_id,
        block_number,
    );
    // [Deploy SimpleWallet Contract] used cycles: 1803600 < 1810K
    helper::check_cycles("Deploy SimpleWallet", run_result.cycles, 2_100_000);

    let account_script = new_contract_account_script(&state, from_id, &from_eth_address, false);
    let contract_id = state
        .get_account_id_by_script_hash(&account_script.hash().into())
        .unwrap()
        .unwrap();
    assert_eq!(contract_id, 6);
}
