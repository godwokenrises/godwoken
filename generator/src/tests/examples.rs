use super::{new_block_info, new_generator, PROXY_PROGRAM_CODE_HASH, SUM_PROGRAM_CODE_HASH};
use crate::smt::{DefaultStore, H256, SMT};
use crate::{Error, Generator, State};
use gw_types::{bytes::Bytes, core::CallType, packed::CallContext, prelude::*};

#[test]
fn test_example_sum() {
    let mut tree: SMT<DefaultStore<H256>> = SMT::default();
    let from_id: u32 = 2;
    let contract_id: u32 = 3;
    let init_value: u64 = 42;

    tree.create_account(contract_id, SUM_PROGRAM_CODE_HASH.clone(), [0u8; 20])
        .expect("create account");

    // run constructor
    {
        let block_info = new_block_info(0, 0, 0);
        let call_context = CallContext::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .call_type(CallType::Construct.into())
            .args(Bytes::from(init_value.to_le_bytes().to_vec()).pack())
            .build();
        let generator = new_generator();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("construct");
        let return_value = {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&run_result.return_data);
            u64::from_le_bytes(buf)
        };
        assert_eq!(return_value, init_value);

        tree.apply(&run_result).expect("update state");
        println!("result {:?}", run_result);
    }

    // run handle message
    {
        let mut sum_value = init_value;
        for (number, add_value) in &[(1u64, 7u64), (2u64, 16u64)] {
            let block_info = new_block_info(0, *number, 0);
            let call_context = CallContext::new_builder()
                .from_id(from_id.pack())
                .to_id(contract_id.pack())
                .call_type(CallType::HandleMessage.into())
                .args(Bytes::from(add_value.to_le_bytes().to_vec()).pack())
                .build();
            let generator = new_generator();
            let run_result = generator
                .execute(&tree, &block_info, &call_context)
                .expect("construct");
            let return_value = {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&run_result.return_data);
                u64::from_le_bytes(buf)
            };
            sum_value += add_value;
            assert_eq!(return_value, sum_value);
            tree.apply(&run_result).expect("update state");
            println!("result {:?}", run_result);
        }
    }
}

#[test]
fn test_example_proxy_sum() {
    let mut tree: SMT<DefaultStore<H256>> = SMT::default();
    let from_id: u32 = 2;
    let contract_id: u32 = 3;
    let init_value: u64 = 42;
    let proxy_contract_id: u32 = 4;

    tree.create_account(contract_id, SUM_PROGRAM_CODE_HASH.clone(), [0u8; 20])
        .expect("create account");
    tree.create_account(
        proxy_contract_id,
        PROXY_PROGRAM_CODE_HASH.clone(),
        [0u8; 20],
    )
    .expect("create account");

    {
        // run sum contract constructor
        let block_info = new_block_info(0, 0, 0);
        let call_context = CallContext::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .call_type(CallType::Construct.into())
            .args(Bytes::from(init_value.to_le_bytes().to_vec()).pack())
            .build();
        let generator = new_generator();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("construct");
        let return_value = {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&run_result.return_data);
            u64::from_le_bytes(buf)
        };
        assert_eq!(return_value, init_value);

        tree.apply(&run_result).expect("update state");
        println!("result {:?}", run_result);

        // run proxy contract constructor
        let block_info = new_block_info(0, 0, 0);
        let call_context = CallContext::new_builder()
            .from_id(from_id.pack())
            .to_id(proxy_contract_id.pack())
            .call_type(CallType::Construct.into())
            .build();
        let generator = new_generator();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("construct");
        assert!(run_result.return_data.is_empty());

        tree.apply(&run_result).expect("update state");
        println!("result {:?}", run_result);
    }

    // invoke sum contract via proxy contract
    {
        let mut sum_value = init_value;
        for (number, add_value) in &[(1u64, 7u64), (2u64, 16u64)] {
            let block_info = new_block_info(0, *number, 0);
            let mut args = contract_id.to_le_bytes().to_vec();
            args.extend_from_slice(&add_value.to_le_bytes());
            let call_context = CallContext::new_builder()
                .from_id(from_id.pack())
                .to_id(proxy_contract_id.pack())
                .call_type(CallType::HandleMessage.into())
                .args(Bytes::from(args).pack())
                .build();
            let generator = new_generator();
            let run_result = generator
                .execute(&tree, &block_info, &call_context)
                .expect("construct");
            let return_value = {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&run_result.return_data);
                u64::from_le_bytes(buf)
            };
            sum_value += add_value;
            assert_eq!(return_value, sum_value);
            tree.apply(&run_result).expect("update state");
            println!("result {:?}", run_result);
        }

        // check sum contract state
        let block_info = new_block_info(0, 42, 0);
        let call_context = CallContext::new_builder()
            .from_id(from_id.pack())
            .to_id(contract_id.pack())
            .call_type(CallType::HandleMessage.into())
            .args(Bytes::from(0u64.to_le_bytes().to_vec()).pack())
            .build();
        let generator = new_generator();
        let run_result = generator
            .execute(&tree, &block_info, &call_context)
            .expect("handle");
        let return_value = {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&run_result.return_data);
            u64::from_le_bytes(buf)
        };
        assert_eq!(sum_value, return_value);
    }
}

#[test]
fn test_example_proxy_recursive() {
    let mut tree: SMT<DefaultStore<H256>> = SMT::default();
    let from_id: u32 = 2;
    let proxy_contract_id: u32 = 4;
    tree.create_account(
        proxy_contract_id,
        PROXY_PROGRAM_CODE_HASH.clone(),
        [0u8; 20],
    )
    .expect("create account");

    // invoke proxy contract
    {
        let block_info = new_block_info(0, 0, 0);
        /* call proxy contract itself */
        let mut args = proxy_contract_id.to_le_bytes().to_vec();
        args.extend_from_slice(&proxy_contract_id.to_le_bytes());
        let call_context = CallContext::new_builder()
            .from_id(from_id.pack())
            .to_id(proxy_contract_id.pack())
            .call_type(CallType::HandleMessage.into())
            .args(Bytes::from(args).pack())
            .build();
        let generator = new_generator();
        let err = generator
            .execute(&tree, &block_info, &call_context)
            .expect_err("handle");
        let err_code = match err {
            Error::InvalidExitCode(code) => code,
            err => panic!("unexpected {:?}", err),
        };
        assert_eq!(err_code, 10);
    }
}
