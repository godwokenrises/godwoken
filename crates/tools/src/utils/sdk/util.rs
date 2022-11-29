use std::{convert::TryInto, ptr, sync::atomic};

use ckb_dao_utils::extract_dao_data;
use ckb_types::{
    core::{Capacity, EpochNumber, EpochNumberWithFraction, HeaderView},
    packed::CellOutput,
    prelude::*,
    H160, H256, U256,
};
use sha3::{Digest, Keccak256};

use crate::utils::sdk::rpc::CkbRpcClient;
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

pub fn get_max_mature_number(rpc_client: &mut CkbRpcClient) -> Result<u64, String> {
    let cellbase_maturity = EpochNumberWithFraction::from_full_value(
        rpc_client
            .get_consensus()
            .map_err(|err| err.to_string())?
            .cellbase_maturity
            .value(),
    );
    let tip_epoch = rpc_client
        .get_tip_header()
        .map(|header| EpochNumberWithFraction::from_full_value(header.inner.epoch.value()))
        .map_err(|err| err.to_string())?;

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
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "Can not get epoch less than current epoch number".to_string())?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::sdk::test_utils::MockRpcResult;
    use ckb_chain_spec::consensus::ConsensusBuilder;
    use ckb_dao_utils::pack_dao_data;
    use ckb_jsonrpc_types::{Consensus, EpochView, HeaderView};
    use ckb_types::{
        bytes::Bytes,
        core::{capacity_bytes, EpochNumberWithFraction, HeaderBuilder},
    };
    use httpmock::prelude::*;

    #[test]
    fn test_minimal_unlock_point() {
        let cases = vec![
            ((5, 5, 1000), (184, 4, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (184, 5, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (184, 6, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 4, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 5, 1000), (5 + 180, 5, 1000)),
            ((5, 5, 1000), (185, 6, 1000), (5 + 180 * 2, 5, 1000)), // 6/1000 > 5/1000
            ((5, 5, 1000), (186, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (186, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (186, 6, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (364, 6, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 4, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 5, 1000), (5 + 180 * 2, 5, 1000)),
            ((5, 5, 1000), (365, 6, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 4, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 5, 1000), (5 + 180 * 3, 5, 1000)),
            ((5, 5, 1000), (366, 6, 1000), (5 + 180 * 3, 5, 1000)),
        ];
        for (deposit_point, prepare_point, expected) in cases {
            let deposit_point =
                EpochNumberWithFraction::new(deposit_point.0, deposit_point.1, deposit_point.2);
            let prepare_point =
                EpochNumberWithFraction::new(prepare_point.0, prepare_point.1, prepare_point.2);
            let expected = EpochNumberWithFraction::new(expected.0, expected.1, expected.2);
            let deposit_header = HeaderBuilder::default()
                .epoch(deposit_point.full_value().pack())
                .build();
            let prepare_header = HeaderBuilder::default()
                .epoch(prepare_point.full_value().pack())
                .build();
            let actual = minimal_unlock_point(&deposit_header, &prepare_header);
            assert_eq!(
                expected, actual,
                "minimal_unlock_point deposit_point: {}, prepare_point: {}, expected: {}, actual: {}",
                deposit_point, prepare_point, expected, actual,
            );
        }
    }

    #[test]
    fn check_withdraw_calculation() {
        let data = Bytes::from(vec![1; 10]);
        let output = CellOutput::new_builder()
            .capacity(capacity_bytes!(1000000).pack())
            .build();

        let (deposit_point, prepare_point) = ((5, 5, 1000), (184, 4, 1000));
        let deposit_number = deposit_point.0 * deposit_point.2 + deposit_point.1;
        let prepare_number = prepare_point.0 * prepare_point.2 + prepare_point.1;
        let deposit_point =
            EpochNumberWithFraction::new(deposit_point.0, deposit_point.1, deposit_point.2);
        let prepare_point =
            EpochNumberWithFraction::new(prepare_point.0, prepare_point.1, prepare_point.2);
        let deposit_header = HeaderBuilder::default()
            .epoch(deposit_point.full_value().pack())
            .number(deposit_number.pack())
            .dao(pack_dao_data(
                10_000_000_000_123_456,
                Default::default(),
                Default::default(),
                Default::default(),
            ))
            .build();
        let prepare_header = HeaderBuilder::default()
            .epoch(prepare_point.full_value().pack())
            .number(prepare_number.pack())
            .dao(pack_dao_data(
                10_000_000_001_123_456,
                Default::default(),
                Default::default(),
                Default::default(),
            ))
            .build();

        let result = calculate_dao_maximum_withdraw4(
            &deposit_header,
            &prepare_header,
            &output,
            Capacity::bytes(data.len()).unwrap().as_u64(),
        );
        assert_eq!(result, 100_000_000_009_999);
    }

    #[test]
    fn test_get_max_mature_number() {
        {
            // cellbase maturity is 4, tip epoch is 3(200/400), so the max mature block number is 0
            let server = MockServer::start();
            let consensus: Consensus = ConsensusBuilder::default()
                .cellbase_maturity(EpochNumberWithFraction::new(4, 0, 1))
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_consensus");
                then.status(200)
                    .body(MockRpcResult::new(consensus).to_json());
            });

            let tip_header: HeaderView = HeaderBuilder::default()
                .epoch(
                    EpochNumberWithFraction::new(3, 200, 400)
                        .full_value()
                        .pack(),
                )
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_tip_header");
                then.status(200)
                    .body(MockRpcResult::new(tip_header).to_json());
            });

            let mut rpc_client = CkbRpcClient::new(server.base_url().as_str());
            assert_eq!(0, get_max_mature_number(&mut rpc_client).unwrap());
        }

        {
            // cellbase maturity is 3(1/3), tip epoch is 3(300/600), epoch 3 starts at block 1800
            // so the max mature block number is 1800 + (600 * 1 / 6) = 1900
            let server = MockServer::start();
            let consensus: Consensus = ConsensusBuilder::default()
                .cellbase_maturity(EpochNumberWithFraction::new(3, 1, 3))
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_consensus");
                then.status(200)
                    .body(MockRpcResult::new(consensus).to_json());
            });

            let tip_header: HeaderView = HeaderBuilder::default()
                .epoch(
                    EpochNumberWithFraction::new(3, 300, 600)
                        .full_value()
                        .pack(),
                )
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_tip_header");
                then.status(200)
                    .body(MockRpcResult::new(tip_header).to_json());
            });

            let epoch3: EpochView = EpochView {
                number: 3.into(),
                start_number: 1800.into(),
                length: 600.into(),
                compact_target: 0.into(),
            };

            server.mock(|when, then| {
                when.method(POST)
                    .path("/")
                    .body_contains("get_epoch_by_number");
                then.status(200).body(MockRpcResult::new(epoch3).to_json());
            });

            let mut rpc_client = CkbRpcClient::new(server.base_url().as_str());
            assert_eq!(1900, get_max_mature_number(&mut rpc_client).unwrap());
        }

        {
            // cellbase maturity is 3(2/3), tip epoch is 105(300/600), epoch 101 starts at block 150000 and length is 1800
            // so the max mature block number is 150000 + (1800 * 5 / 6) = 151500
            let server = MockServer::start();
            let consensus: Consensus = ConsensusBuilder::default()
                .cellbase_maturity(EpochNumberWithFraction::new(3, 2, 3))
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_consensus");
                then.status(200)
                    .body(MockRpcResult::new(consensus).to_json());
            });

            let tip_header: HeaderView = HeaderBuilder::default()
                .epoch(
                    EpochNumberWithFraction::new(105, 300, 600)
                        .full_value()
                        .pack(),
                )
                .build()
                .into();
            server.mock(|when, then| {
                when.method(POST).path("/").body_contains("get_tip_header");
                then.status(200)
                    .body(MockRpcResult::new(tip_header).to_json());
            });

            let epoch3: EpochView = EpochView {
                number: 101.into(),
                start_number: 150000.into(),
                length: 1800.into(),
                compact_target: 0.into(),
            };

            server.mock(|when, then| {
                when.method(POST)
                    .path("/")
                    .body_contains("get_epoch_by_number");
                then.status(200).body(MockRpcResult::new(epoch3).to_json());
            });

            let mut rpc_client = CkbRpcClient::new(server.base_url().as_str());
            assert_eq!(151500, get_max_mature_number(&mut rpc_client).unwrap());
        }
    }
}
