//! Test simple transfer
//!   See ./evm-contracts/SimpleTransfer.sol

use crate::helper::{
    self, deploy, new_contract_account_script, new_contract_account_script_with_nonce, setup,
    CREATOR_ACCOUNT_ID,
};
use gw_common::state::State;
use gw_generator::traits::StateExt;

const SS_INIT_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");

#[test]
fn test_account_already_exists() {
    let (store, mut state, generator) = setup();
    let block_producer_id = helper::create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        helper::create_eth_eoa_account(&mut state, &from_eth_address, 400000u64.into());

    let created_ss_account_script = new_contract_account_script_with_nonce(&from_eth_address, 0);
    let created_ss_account_id = state
        .create_account_from_script(created_ss_account_script)
        .unwrap();

    // Deploy SimpleStorage
    let _run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        SS_INIT_CODE,
        77659,
        0,
        block_producer_id,
        0,
    );
    let ss_account_script = new_contract_account_script(&state, from_id, &from_eth_address, false);
    let ss_account_id = state
        .get_account_id_by_script_hash(&ss_account_script.hash().into())
        .unwrap()
        .unwrap();
    assert_eq!(created_ss_account_id, ss_account_id);
}
