use std::{convert::TryInto, ptr, sync::atomic};

use anyhow::{Context, Result};
use ckb_dao_utils::extract_dao_data;
use ckb_types::{
    core::{Capacity, EpochNumber, EpochNumberWithFraction, HeaderView},
    packed::CellOutput,
    prelude::*,
    H160, H256, U256,
};
use gw_rpc_client::ckb_client::CkbClient;
use sha3::{Digest, Keccak256};

use crate::utils::sdk::traits::LiveCell;

pub fn zeroize_privkey(key: &mut secp256k1::SecretKey) {
    let key_ptr = key.as_mut_ptr();
    for i in 0..key.len() as isize {
        unsafe { ptr::write_volatile(key_ptr.offset(i), Default::default()) }
        atomic::compiler_fence(atomic::Ordering::SeqCst);
    }
}

pub fn zeroize_slice(data: &mut [u8]) {
    for elem in data {
        unsafe { ptr::write_volatile(elem, Default::default()) }
        atomic::compiler_fence(atomic::Ordering::SeqCst);
    }
}

pub async fn get_max_mature_number(rpc_client: &CkbClient) -> Result<u64> {
    let cellbase_maturity = EpochNumberWithFraction::from_full_value(
        rpc_client.get_consensus().await?.cellbase_maturity.value(),
    );
    let tip_epoch = rpc_client
        .get_tip_header()
        .await
        .map(|header| EpochNumberWithFraction::from_full_value(header.inner.epoch.value()))?;

    let tip_epoch_rational = tip_epoch.to_rational();
    let cellbase_maturity_rational = cellbase_maturity.to_rational();

    if tip_epoch_rational < cellbase_maturity_rational {
        // No cellbase live cell is mature
        Ok(0)
    } else {
        let difference = tip_epoch_rational - cellbase_maturity_rational;
        let rounds_down_difference = difference.clone().into_u256();
        let difference_delta = difference - rounds_down_difference.clone();

        let epoch_number = u64::from_le_bytes(
            rounds_down_difference.to_le_bytes()[..8]
                .try_into()
                .expect("should be u64"),
        )
        .into();
        let max_mature_epoch = rpc_client
            .get_epoch_by_number(epoch_number)
            .await?
            .context("Can not get epoch less than current epoch number")?;

        let max_mature_block_number = (difference_delta
            * U256::from(max_mature_epoch.length.value())
            + U256::from(max_mature_epoch.start_number.value()))
        .into_u256();

        Ok(u64::from_le_bytes(
            max_mature_block_number.to_le_bytes()[..8]
                .try_into()
                .expect("should be u64"),
        ))
    }
}

pub fn is_mature(info: &LiveCell, max_mature_number: u64) -> bool {
    // Not cellbase cell
    info.tx_index > 0
    // Live cells in genesis are all mature
        || info.block_number == 0
        || info.block_number <= max_mature_number
}

pub fn minimal_unlock_point(
    deposit_header: &HeaderView,
    prepare_header: &HeaderView,
) -> EpochNumberWithFraction {
    const LOCK_PERIOD_EPOCHES: EpochNumber = 180;

    // https://github.com/nervosnetwork/ckb-system-scripts/blob/master/c/dao.c#L182-L223
    let deposit_point = deposit_header.epoch();
    let prepare_point = prepare_header.epoch();
    let prepare_fraction = prepare_point.index() * deposit_point.length();
    let deposit_fraction = deposit_point.index() * prepare_point.length();
    let passed_epoch_cnt = if prepare_fraction > deposit_fraction {
        prepare_point.number() - deposit_point.number() + 1
    } else {
        prepare_point.number() - deposit_point.number()
    };
    let rest_epoch_cnt =
        (passed_epoch_cnt + (LOCK_PERIOD_EPOCHES - 1)) / LOCK_PERIOD_EPOCHES * LOCK_PERIOD_EPOCHES;
    EpochNumberWithFraction::new(
        deposit_point.number() + rest_epoch_cnt,
        deposit_point.index(),
        deposit_point.length(),
    )
}

pub fn calculate_dao_maximum_withdraw4(
    deposit_header: &HeaderView,
    prepare_header: &HeaderView,
    output: &CellOutput,
    occupied_capacity: u64,
) -> u64 {
    let (deposit_ar, _, _, _) = extract_dao_data(deposit_header.dao());
    let (prepare_ar, _, _, _) = extract_dao_data(prepare_header.dao());
    let output_capacity: Capacity = output.capacity().unpack();
    let counted_capacity = output_capacity.as_u64() - occupied_capacity;
    let withdraw_counted_capacity =
        u128::from(counted_capacity) * u128::from(prepare_ar) / u128::from(deposit_ar);
    occupied_capacity + withdraw_counted_capacity as u64
}

pub fn serialize_signature(signature: &secp256k1::ecdsa::RecoverableSignature) -> [u8; 65] {
    let (recov_id, data) = signature.serialize_compact();
    let mut signature_bytes = [0u8; 65];
    signature_bytes[0..64].copy_from_slice(&data[0..64]);
    signature_bytes[64] = recov_id.to_i32() as u8;
    signature_bytes
}

pub fn blake160(message: &[u8]) -> H160 {
    let r = ckb_hash::blake2b_256(message);
    H160::from_slice(&r[..20]).unwrap()
}

/// Do an ethereum style public key hash.
pub fn keccak160(message: &[u8]) -> H160 {
    let mut hasher = Keccak256::new();
    hasher.update(message);
    let r = hasher.finalize();
    H160::from_slice(&r[12..]).unwrap()
}

/// Do an ethereum style message convert before do a signature.
pub fn convert_keccak256_hash(message: &[u8]) -> H256 {
    let eth_prefix: &[u8; 28] = b"\x19Ethereum Signed Message:\n32";
    let mut hasher = Keccak256::new();
    hasher.update(eth_prefix);
    hasher.update(message);
    let r = hasher.finalize();
    H256::from_slice(r.as_slice()).expect("convert_keccak256_hash")
}
