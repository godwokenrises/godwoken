use exit_withdraw_state_validator::{
    settlement::{codec, SettlementAction, MAX_RECORDS},
    v0, wire, Error,
};
use gw_types::prelude::*;

#[test]
fn unknown_operations_rejected_before_processing_record_payloads() {
    // Oversized but structurally valid record list: unknown operations must fail at dispatch,
    // before the bounded record/proof loaders even examine or copy these payloads.
    for op in [1u32, 2, 6, 8, 11, u32::MAX] {
        let body = wire::SettlementAction::new_builder()
            .version(wire::Uint32::from_slice(&3u32.to_le_bytes()).unwrap())
            .operation(wire::Uint32::from_slice(&op.to_le_bytes()).unwrap())
            .records(
                wire::SettlementRecords::new_builder()
                    .set(vec![wire::Bytes::default(); MAX_RECORDS + 1])
                    .build(),
            )
            .build();
        for (tag, decoder) in [
            (
                codec::V1_ACTION_TAG,
                codec::decode_action as fn(&[u8]) -> Result<SettlementAction, Error>,
            ),
            (
                codec::V0_ACTION_TAG,
                v0::decode_action as fn(&[u8]) -> Result<SettlementAction, Error>,
            ),
        ] {
            let bytes = [tag.to_le_bytes().as_slice(), body.as_slice()].concat();
            assert_eq!(decoder(&bytes).unwrap_err(), Error::Encoding);
        }
    }
}
