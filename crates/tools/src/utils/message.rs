use anyhow::anyhow;
use ckb_fixed_hash::H256;
use gw_generator::account_lock_manage::eip712::{self, types::EIP712Domain};
use gw_types::{packed::RawL2Transaction, prelude::*};

use crate::hasher::{CkbHasher, EthHasher};

fn domain_with_chain_id(chain_id: u64) -> EIP712Domain {
    EIP712Domain {
        name: "Godwoken".to_string(),
        chain_id,
        version: "1".to_string(),
        verifying_contract: None,
        salt: None,
    }
}

pub fn generate_transaction_message_to_sign(
    raw_l2transaction: &RawL2Transaction,
    rollup_type_hash: &H256,
    sender_script_hash: &H256,
    receiver_script_hash: &H256,
) -> H256 {
    let raw_data = raw_l2transaction.as_slice();
    let rollup_type_hash_data = rollup_type_hash.as_bytes();

    let digest = CkbHasher::new()
        .update(rollup_type_hash_data)
        .update(sender_script_hash.as_bytes())
        .update(receiver_script_hash.as_bytes())
        .update(raw_data)
        .finalize();

    let message = EthHasher::new()
        .update("\x19Ethereum Signed Message:\n32")
        .update(digest.as_bytes())
        .finalize();

    message
}

pub fn generate_eip712_message_to_sign(
    raw_l2transaction: RawL2Transaction,
    sender_address: gw_common::registry_address::RegistryAddress,
    receiver_script_hash: gw_common::H256,
    chain_id: u64,
) -> H256 {
    let typed_tx = eip712::types::L2Transaction::from_raw(
        raw_l2transaction,
        sender_address,
        receiver_script_hash,
    )
    .map_err(|err| {
        anyhow!(format!("Invalid l2transaction format {}", err));
    })
    .expect("l2transaction");

    eip712::traits::EIP712Encode::eip712_message(
        &typed_tx,
        eip712::traits::EIP712Encode::hash_struct(&domain_with_chain_id(chain_id)),
    )
    .into()
}
