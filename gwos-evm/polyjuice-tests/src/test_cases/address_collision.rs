use std::convert::TryInto;

use anyhow::Result;
use ckb_vm::Bytes;
use gw_common::{
    builtins::ETH_REGISTRY_ACCOUNT_ID, registry_address::RegistryAddress, state::State,
};
use gw_store::{chain_view::ChainView, state::traits::JournalDB, traits::chain_store::ChainStore};
use gw_types::{
    packed::RawL2Transaction,
    prelude::{Builder, Entity, Pack, CalcHash},
};

use crate::helper::{
    compute_create2_script, contract_script_to_eth_addr, create_block_producer,
    create_eth_eoa_account, deploy, new_block_info, setup, MockContractInfo, PolyjuiceArgsBuilder,
    CREATOR_ACCOUNT_ID, L2TX_MAX_CYCLES,
};

const INIT_CODE: &str = include_str!("./evm-contracts/CreateContract.bin");
const SS_CODE: &str = include_str!("./evm-contracts/SimpleStorage.bin");
const CREATE2_IMPL_CODE: &str = include_str!("./evm-contracts/Create2Impl.bin");

#[test]
fn create_address_collision_overwrite() -> Result<()> {
    let (store, mut state, generator) = setup();
    let block_producer_id = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    let create_eth_addr = hex::decode("808bfd2069b1ca619a55585e7b1ac1b11d392af9")?;
    let create_eth_reg_addr =
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, create_eth_addr.clone());
    //create EOA account with create_account address first
    let (eoa_id, _) = create_eth_eoa_account(
        &mut state,
        &create_eth_addr.try_into().unwrap(),
        200000u64.into(),
    );

    assert_eq!(eoa_id, 6);

    let _ = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        INIT_CODE,
        170000,
        0,
        block_producer_id,
        1,
    );

    let script_hash = state.get_script_hash_by_registry_address(&create_eth_reg_addr)?;
    assert!(script_hash.is_some());
    let create_account_id = state.get_account_id_by_script_hash(&script_hash.unwrap())?;
    assert_eq!(create_account_id, Some(8));
    Ok(())
}

#[test]
fn create_address_collision_duplicate() {
    let (store, mut state, generator) = setup();
    let block_producer_id = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    let create_eth_addr = hex::decode("808bfd2069b1ca619a55585e7b1ac1b11d392af9").unwrap();
    //create EOA account with create_account address first
    let (eoa_id, _) = create_eth_eoa_account(
        &mut state,
        &create_eth_addr.try_into().unwrap(),
        200000u64.into(),
    );

    assert_eq!(eoa_id, 6);

    let _ = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        eoa_id,
        SS_CODE,
        130000,
        0,
        block_producer_id.clone(),
        1,
    );
    let eoa_nonce = state.get_nonce(eoa_id);
    assert_eq!(eoa_nonce, Ok(1));

    let run_result = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        INIT_CODE,
        130000,
        0,
        block_producer_id,
        1,
    );

    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);
}

#[test]
fn create2_address_collision_overwrite() -> Result<()> {
    let (store, mut state, generator) = setup();
    let block_producer_id = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    let create2_eth_addr = hex::decode("d78e81d86aeace84ff6311db7b134c1231a4a402")?;
    let create2_eth_reg_addr =
        RegistryAddress::new(ETH_REGISTRY_ACCOUNT_ID, create2_eth_addr.clone());
    //create EOA account with create_account address first
    let (eoa_id, _) = create_eth_eoa_account(
        &mut state,
        &create2_eth_addr.try_into().unwrap(),
        200000u64.into(),
    );

    assert_eq!(eoa_id, 6);

    let _ = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        CREATE2_IMPL_CODE,
        122000,
        0,
        block_producer_id.clone(),
        1,
    );

    let create2_contract = MockContractInfo::create(&from_eth_address, 0);
    let create2_contract_script_hash = create2_contract.script_hash;
    let create2_contract_id = state
        .get_account_id_by_script_hash(&create2_contract_script_hash)
        .unwrap()
        .unwrap();
    let input_value_u128: u128 = 0x9a;
    // bytes32 salt
    let input_salt = "1111111111111111111111111111111111111111111111111111111111111111";
    // Create2Impl.deploy(uint256 value, bytes32 salt, bytes memory code)
    let block_number = 2;
    let block_info = new_block_info(block_producer_id, block_number, block_number);

    //consturct input:
    //0x9a
    //input_salt
    //SS_INIT_CODE
    let input = hex::decode("66cfa057000000000000000000000000000000000000000000000000000000000000009a1111111111111111111111111111111111111111111111111111111111111111000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000ea6080604052607b60008190555060d08061001a6000396000f3fe60806040526004361060295760003560e01c806360fe47b11460345780636d4ce63c14605f57602f565b36602f57005b600080fd5b605d60048036036020811015604857600080fd5b81019080803590602001909291905050506087565b005b348015606a57600080fd5b5060716091565b6040518082815260200191505060405180910390f35b8060008190555050565b6000805490509056fea2646970667358221220b796688cdcda21059332f8ef75088337063fcf7a8ab96bb23bc06ec8623d679064736f6c6343000602003300000000000000000000000000000000000000000000")?;

    // Create2Impl.deploy(uint256 value, bytes32 salt, bytes memory code)
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
    state.finalise().expect("update state");

    let create2_script = compute_create2_script(
        create2_contract.eth_addr.as_slice(),
        &hex::decode(input_salt).unwrap()[..],
        &hex::decode(SS_CODE).unwrap()[..],
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

    let script_hash = state.get_script_hash_by_registry_address(&create2_eth_reg_addr)?;
    assert!(script_hash.is_some());
    let create_account_id = state.get_account_id_by_script_hash(&create2_script_hash.into())?;
    assert_eq!(create_account_id, Some(8));
    Ok(())
}

#[test]
fn create2_address_collision_duplicate() -> Result<()> {
    let (store, mut state, generator) = setup();
    let block_producer_id = create_block_producer(&mut state);

    let from_eth_address = [1u8; 20];
    let (from_id, _from_script_hash) =
        create_eth_eoa_account(&mut state, &from_eth_address, 200000u64.into());

    let create2_eth_addr = hex::decode("9267e505e0af739a9c434744d14a442792be98ef")?;
    //create EOA account with create_account address first
    let (eoa_id, _) = create_eth_eoa_account(
        &mut state,
        &create2_eth_addr.try_into().unwrap(),
        200000u64.into(),
    );

    assert_eq!(eoa_id, 6);
    let _ = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        eoa_id,
        SS_CODE,
        122000,
        0,
        block_producer_id.clone(),
        1,
    );
    let eoa_nonce = state.get_nonce(eoa_id);
    assert_eq!(eoa_nonce, Ok(1));

    let _ = deploy(
        &generator,
        &store,
        &mut state,
        CREATOR_ACCOUNT_ID,
        from_id,
        CREATE2_IMPL_CODE,
        122000,
        0,
        block_producer_id.clone(),
        1,
    );

    let create2_contract = MockContractInfo::create(&from_eth_address, 0);
    let create2_contract_script_hash = create2_contract.script_hash;
    let create2_contract_id = state
        .get_account_id_by_script_hash(&create2_contract_script_hash)?
        .unwrap();
    let input_value_u128: u128 = 0x9a;

    //consturct input:
    //0x9a
    //input_salt "1111111111111111111111111111111111111111111111111111111111111111"
    //SS_INIT_CODE
    // Create2Impl.deploy(uint256 value, bytes32 salt, bytes memory code)
    let block_number = 2;
    let block_info = new_block_info(block_producer_id, block_number, block_number);
    let input = hex::decode("66cfa057000000000000000000000000000000000000000000000000000000000000009a1111111111111111111111111111111111111111111111111111111111111111000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000ea6080604052607b60008190555060d08061001a6000396000f3fe60806040526004361060295760003560e01c806360fe47b11460345780636d4ce63c14605f57602f565b36602f57005b600080fd5b605d60048036036020811015604857600080fd5b81019080803590602001909291905050506087565b005b348015606a57600080fd5b5060716091565b6040518082815260200191505060405180910390f35b8060008190555050565b6000805490509056fea2646970667358221220b796688cdcda21059332f8ef75088337063fcf7a8ab96bb23bc06ec8623d679064736f6c6343000602003300000000000000000000000000000000000000000000").unwrap();

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
    let run_result = generator.execute_transaction(
        &ChainView::new(&db, tip_block_hash),
        &mut state,
        &block_info,
        &raw_tx,
        L2TX_MAX_CYCLES,
        None,
    )?;
    assert_eq!(run_result.exit_code, crate::constant::EVMC_REVERT);

    Ok(())
}
