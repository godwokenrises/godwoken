//! Shared v0/v1 shutdown attacks. Signatures are supplied by each protocol's real fixture.
use super::*;

pub(super) fn forbidden_actions(good: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    use packed::RollupActionUnion::*;
    let legacy = [
        (
            "legacy submit block (including deposits/L2 transfers/withdrawals)",
            RollupSubmitBlock(packed::RollupSubmitBlock::default()),
        ),
        (
            "legacy enter challenge",
            RollupEnterChallenge(packed::RollupEnterChallenge::default()),
        ),
        (
            "legacy cancel challenge",
            RollupCancelChallenge(packed::RollupCancelChallenge::default()),
        ),
        (
            "legacy revert",
            RollupRevert(packed::RollupRevert::default()),
        ),
    ];
    let mut cases: Vec<_> = legacy
        .into_iter()
        .map(|(name, action)| {
            let bytes = packed::RollupAction::new_builder()
                .set(action)
                .build()
                .as_slice()
                .to_vec();
            // Valid Molecule action encoding; rejection must be the refund entry's action gate.
            packed::RollupAction::from_slice(&bytes).unwrap();
            (name, bytes)
        })
        .collect();
    let native = if good[..4] == contract::settlement::codec::V1_ACTION_TAG.to_le_bytes() {
        contract::settlement::codec::decode_action(good)
            .unwrap()
            .native
    } else {
        contract::v0::decode_action(good).unwrap().native
    };
    cases.push((
        "retired direct-account exit",
        contract::codec::encode_action(&native),
    ));
    let mut other = good.to_vec();
    other[..4].copy_from_slice(&if good[0] == 5 { 6u32 } else { 5u32 }.to_le_bytes());
    cases.push(("other protocol envelope", other));
    let mut unknown = good.to_vec();
    unknown[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    cases.push(("unknown outer action", unknown));
    for (name, op) in [
        ("unknown inner operation", 11u32),
        ("unknown inner operation max", u32::MAX),
    ] {
        let body = contract::wire::SettlementAction::from_slice(&good[4..])
            .unwrap()
            .as_builder()
            .operation(contract::wire::Uint32::from_slice(&op.to_le_bytes()).unwrap())
            .build();
        cases.push((name, [&good[..4], body.as_slice()].concat()));
    }
    let mut trailing = good.to_vec();
    trailing.push(0);
    cases.push(("trailing action bytes", trailing));
    cases.push(("empty action", vec![]));
    cases
}

pub(super) fn set_payload(tx: &mut Transaction, payload: &[u8]) {
    let w = packed::WitnessArgs::from_slice(&bytes(&tx.witnesses[0]).unwrap()).unwrap();
    tx.witnesses[0] = hx(w
        .as_builder()
        .output_type(Some(gw_types::bytes::Bytes::copy_from_slice(payload)).pack())
        .build()
        .as_slice());
}

pub(super) fn frozen_state_attacks(tx: &Transaction) -> Vec<(&'static str, Transaction)> {
    let state = packed::GlobalState::from_slice(&bytes(&tx.outputs_data[0]).unwrap()).unwrap();
    let mutations = [
        (
            "config replacement",
            state
                .clone()
                .as_builder()
                .rollup_config_hash([71; 32].pack())
                .build(),
        ),
        (
            "block root replacement",
            state
                .clone()
                .as_builder()
                .block(
                    state
                        .block()
                        .as_builder()
                        .merkle_root([72; 32].pack())
                        .build(),
                )
                .build(),
        ),
        (
            "block production",
            state
                .clone()
                .as_builder()
                .block(
                    state
                        .block()
                        .as_builder()
                        .count(
                            (u64::from_le_bytes(
                                state.block().count().as_slice().try_into().unwrap(),
                            ) + 1)
                                .pack(),
                        )
                        .build(),
                )
                .build(),
        ),
        (
            "revert state replacement",
            state
                .clone()
                .as_builder()
                .reverted_block_root([73; 32].pack())
                .build(),
        ),
        (
            "tip replacement",
            state
                .clone()
                .as_builder()
                .tip_block_hash([74; 32].pack())
                .build(),
        ),
        (
            "timestamp advance",
            state
                .clone()
                .as_builder()
                .tip_block_timestamp(1u64.pack())
                .build(),
        ),
        (
            "finality advance",
            state
                .clone()
                .as_builder()
                .last_finalized_timepoint(1u64.pack())
                .build(),
        ),
        (
            "challenge status change",
            state.clone().as_builder().status(1u8.into()).build(),
        ),
        (
            "protocol version change",
            state.clone().as_builder().version(2u8.into()).build(),
        ),
        (
            "account creation",
            state
                .clone()
                .as_builder()
                .account(
                    state
                        .account()
                        .as_builder()
                        .count(
                            (u32::from_le_bytes(
                                state.account().count().as_slice().try_into().unwrap(),
                            ) + 1)
                                .pack(),
                        )
                        .build(),
                )
                .build(),
        ),
        (
            "unproven L2 state replacement",
            state
                .clone()
                .as_builder()
                .account(
                    state
                        .account()
                        .as_builder()
                        .merkle_root([75; 32].pack())
                        .build(),
                )
                .build(),
        ),
    ];
    let mut cases = Vec::new();
    for (name, s) in mutations {
        assert_ne!(
            s.as_slice(),
            state.as_slice(),
            "ineffective mutation {name}"
        );
        let mut bad = tx.clone();
        bad.outputs_data[0] = hx(s.as_slice());
        cases.push((name, bad));
    }
    let mut deleted = tx.clone();
    deleted.outputs.remove(0);
    deleted.outputs_data.remove(0);
    cases.push(("rollup destruction", deleted));
    let mut duplicate = tx.clone();
    duplicate.outputs.push(duplicate.outputs[0].clone());
    duplicate
        .outputs_data
        .push(duplicate.outputs_data[0].clone());
    cases.push(("rollup duplication", duplicate));
    let mut fallback = tx.clone();
    let w = packed::WitnessArgs::from_slice(&bytes(&fallback.witnesses[0]).unwrap()).unwrap();
    let payload = w.output_type();
    fallback.witnesses[0] = hx(w
        .as_builder()
        .output_type(packed::BytesOpt::default())
        .input_type(payload)
        .build()
        .as_slice());
    cases.push(("action in wrong witness field", fallback));
    cases
}

pub(super) fn assert_type_rejection(node: &Node, tx: &Transaction, label: &str, code: Option<i8>) {
    let err = node.vm(tx).expect_err(label).to_string();
    assert!(
        err.contains("Inputs[0].Type") && err.contains("ValidationFailure"),
        "{label}: must fail refund type script, not signature: {err}"
    );
    if let Some(code) = code {
        assert!(
            err.contains(&format!("error code {code} on")),
            "{label}: wrong error: {err}"
        );
    }
    println!("Refund-only gate rejected {label}");
}
