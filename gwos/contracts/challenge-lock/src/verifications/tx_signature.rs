use crate::verifications::context::{verify_tx_context, TxContext, TxContextInput};
use crate::verifications::eip712::{traits::EIP712Encode, types::EIP712Domain};
use alloc::vec;
use core::result::Result;
use gw_state::{ckb_smt::smt::Pair, constants::GW_MAX_KV_PAIRS, kv_state::KVState};
use gw_types::{
    packed::{ChallengeLockArgs, RollupConfig},
    prelude::*,
};
use gw_utils::{
    ckb_std::{
        ckb_constants::Source,
        ckb_types::{bytes::Bytes, prelude::Unpack as CKBUnpack},
        high_level::load_witness_args,
    },
    error::Error,
    gw_types::{
        core::SigningType,
        packed::{
            CCTransactionSignatureWitness, CCTransactionSignatureWitnessReader, Script, ScriptVec,
        },
    },
    signature::check_l2_account_signature_cell,
};
use gw_utils::{
    gw_common::H256,
    gw_types::{self, packed::RawL2Transaction},
};
use sha3::{Digest, Keccak256};

/// Verify tx signature
pub fn verify_tx_signature(
    _rollup_script_hash: &[u8; 32],
    rollup_config: &RollupConfig,
    lock_args: &ChallengeLockArgs,
) -> Result<(), Error> {
    let witness_args: Bytes = load_witness_args(0, Source::GroupInput)?
        .lock()
        .to_opt()
        .ok_or(Error::InvalidArgs)?
        .unpack();
    let unlock_args = match CCTransactionSignatureWitnessReader::verify(&witness_args, false) {
        Ok(_) => CCTransactionSignatureWitness::new_unchecked(witness_args),
        Err(_) => return Err(Error::InvalidArgs),
    };
    let tx = unlock_args.l2tx();

    // check rollup chain id
    let expected_rollup_chain_id: u64 = rollup_config.chain_id().unpack();
    let chain_id: u64 = tx.raw().chain_id().unpack();
    if expected_rollup_chain_id != chain_id {
        crate::ckb_std::debug!("Tx using wrong rollup_chain_id");
        return Err(Error::WrongSignature);
    }

    let account_count: u32 = unlock_args.account_count().unpack();
    let mut tree_buffer = [Pair::default(); GW_MAX_KV_PAIRS];
    let kv_state_proof: Bytes = unlock_args.kv_state_proof().unpack();

    let kv_state = KVState::build(
        &mut tree_buffer,
        unlock_args.kv_state().as_reader(),
        &kv_state_proof,
        account_count,
        None,
    )?;

    let scripts = ScriptVec::new_builder()
        .push(unlock_args.sender())
        .push(unlock_args.receiver())
        .build();
    let target = lock_args.target();
    let raw_block = unlock_args.raw_l2block();
    let tx_proof = unlock_args.tx_proof();
    let raw_tx = tx.raw();

    let input = TxContextInput {
        tx,
        kv_state,
        scripts,
        raw_block,
        rollup_config,
        target,
        tx_proof,
    };

    let TxContext {
        sender_script_hash,
        receiver_script_hash,
        receiver,
        sender: _,
        sender_address,
    } = verify_tx_context(input)?;

    let (message, signing_type) = match try_assemble_polyjuice_args(&raw_tx, receiver) {
        Some(rlp_data) => {
            let mut hasher = Keccak256::new();
            hasher.update(&*rlp_data);
            let buf = hasher.finalize();
            let mut signing_message = [0u8; 32];
            signing_message.copy_from_slice(&buf[..]);
            (H256::from(signing_message), SigningType::Raw)
        }
        None => {
            let chain_id = raw_tx.chain_id().unpack();
            let typed_tx = crate::verifications::eip712::types::L2Transaction::from_raw(
                raw_tx,
                sender_address,
                receiver_script_hash,
            )?;
            let message =
                typed_tx.eip712_message(EIP712Domain::domain_with_chain_id(chain_id).hash_struct());
            (message.into(), SigningType::Raw)
        }
    };

    // verify sender's script is in the input
    check_l2_account_signature_cell(&sender_script_hash, signing_type, message)?;
    Ok(())
}

fn try_assemble_polyjuice_args(
    raw_tx: &RawL2Transaction,
    receiver_script: Script,
) -> Option<Bytes> {
    let args: Bytes = raw_tx.args().unpack();
    if args.len() < 52 {
        return None;
    }
    if args[0..7] != b"\xFF\xFF\xFFPOLY"[..] {
        return None;
    }
    let mut stream = rlp::RlpStream::new();
    stream.begin_unbounded_list();
    let nonce: u32 = raw_tx.nonce().unpack();
    stream.append(&nonce);
    let gas_price = {
        let mut data = [0u8; 16];
        data.copy_from_slice(&args[16..32]);
        u128::from_le_bytes(data)
    };
    stream.append(&gas_price);
    let gas_limit = {
        let mut data = [0u8; 8];
        data.copy_from_slice(&args[8..16]);
        u64::from_le_bytes(data)
    };
    stream.append(&gas_limit);
    let to = if args[7] == 3 {
        // 3 for EVMC_CREATE
        vec![0u8; 0]
    } else {
        // For contract calling, chain id is read from scrpit args of
        // receiver_script, see the following link for more details:
        // https://github.com/nervosnetwork/godwoken-polyjuice#normal-contract-account-script
        if receiver_script.args().len() < 36 {
            return None;
        }
        let mut to = vec![0u8; 20];
        let receiver_hash = receiver_script.hash();
        to[0..16].copy_from_slice(&receiver_hash[0..16]);
        let to_id: u32 = raw_tx.to_id().unpack();
        to[16..20].copy_from_slice(&to_id.to_le_bytes());
        to
    };
    stream.append(&to);
    let value = {
        let mut data = [0u8; 16];
        data.copy_from_slice(&args[32..48]);
        u128::from_le_bytes(data)
    };
    stream.append(&value);
    let payload_length = {
        let mut data = [0u8; 4];
        data.copy_from_slice(&args[48..52]);
        u32::from_le_bytes(data)
    } as usize;
    if args.len() != 52 + payload_length {
        return None;
    }
    stream.append(&args[52..52 + payload_length].to_vec());
    let chain_id: u64 = raw_tx.chain_id().unpack();
    stream.append(&chain_id);
    stream.append(&0u8);
    stream.append(&0u8);
    stream.finalize_unbounded_list();
    Some(Bytes::from(stream.out().to_vec()))
}
