use super::new_block_info;
use crate::{
    account_lock_manage::AccountLockManage, backend_manage::BackendManage, dummy_state::DummyState,
    error::TransactionError, syscalls::ERROR_DUPLICATED_SCRIPT_HASH, traits::StateExt, Generator,
};
use core::panic;
use gw_common::builtin_scripts::META_CONTRACT_VALIDATOR_CODE_HASH;
use gw_common::state::State;
use gw_common::{CodeStore, H256};
use gw_store::Store;
use gw_types::{
    packed::{BlockInfo, CreateAccount, MetaContractArgs, RawL2Transaction, Script},
    prelude::*,
};

fn run_contract<S: State + CodeStore>(
    store: &Store,
    tree: &mut S,
    from_id: u32,
    to_id: u32,
    args: MetaContractArgs,
    block_info: &BlockInfo,
) -> Result<Vec<u8>, TransactionError> {
    let raw_tx = RawL2Transaction::new_builder()
        .from_id(from_id.pack())
        .to_id(to_id.pack())
        .args(args.as_bytes().pack())
        .build();
    let backend_manage = BackendManage::default();
    let account_lock_manage = AccountLockManage::default();
    let generator = Generator::new(backend_manage, account_lock_manage, Default::default());
    let run_result = generator.execute(store, tree, block_info, &raw_tx)?;
    tree.apply_run_result(&run_result).expect("update state");
    Ok(run_result.return_data)
}

#[test]
fn test_meta_contract() {
    let store = Store::open_tmp().unwrap();
    let mut tree = DummyState::default();
    // init accounts
    let meta_contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(Into::<[u8; 32]>::into(META_CONTRACT_VALIDATOR_CODE_HASH.clone()).pack())
                .args([0u8; 32].to_vec().pack())
                .build(),
        )
        .expect("create account");
    assert_eq!(meta_contract_id, 0);

    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");

    let block_info = new_block_info(a_id, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args(vec![42].pack())
        .build();
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .build(),
        )
        .build();
    let return_data = run_contract(&store, &mut tree, a_id, meta_contract_id, args, &block_info)
        .expect("execute");
    let account_id = {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&return_data);
        u32::from_le_bytes(buf)
    };
    assert_ne!(account_id, 0);

    let script_hash = tree.get_script_hash(account_id).expect("get script hash");
    assert_ne!(script_hash, H256::zero(), "script hash must exists");
    assert_eq!(
        script_hash,
        contract_script.hash().into(),
        "script hash must according to create account"
    );
}

#[test]
fn test_duplicated_script_hash() {
    let store = Store::open_tmp().unwrap();
    let mut tree = DummyState::default();
    // init accounts
    let meta_contract_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash(Into::<[u8; 32]>::into(META_CONTRACT_VALIDATOR_CODE_HASH.clone()).pack())
                .args([0u8; 32].to_vec().pack())
                .build(),
        )
        .expect("create account");
    assert_eq!(meta_contract_id, 0);

    let a_id = tree
        .create_account_from_script(
            Script::new_builder()
                .code_hash([0u8; 32].pack())
                .args([0u8; 20].to_vec().pack())
                .build(),
        )
        .expect("create account");

    let block_info = new_block_info(a_id, 1, 0);

    // create contract
    let contract_script = Script::new_builder()
        .code_hash([0u8; 32].pack())
        .args(vec![42].pack())
        .build();

    let _id = tree
        .create_account_from_script(contract_script.clone())
        .expect("create account");

    // should return duplicated script hash
    let args = MetaContractArgs::new_builder()
        .set(
            CreateAccount::new_builder()
                .script(contract_script.clone())
                .build(),
        )
        .build();
    let err =
        run_contract(&store, &mut tree, a_id, meta_contract_id, args, &block_info).unwrap_err();
    let err_code = match err {
        TransactionError::InvalidExitCode(code) => code,
        err => panic!("unexpected {:?}", err),
    };
    assert_eq!(err_code, ERROR_DUPLICATED_SCRIPT_HASH as i8);
}
