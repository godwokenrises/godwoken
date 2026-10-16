#[path = "native_support.rs"]
mod support;
use crate::*;
use alloc::vec;
use gw_common::state::*;
use gw_hash::blake2b::hash;
use gw_types::{packed, prelude::*, U256};
use support::*;

#[test]
fn batch_debits_token_and_capacity_with_real_proof() {
    let f = Fixture::new();
    let r = f.verify().unwrap();
    assert_eq!(r.debits[&(0, 3)], 10.into());
    assert_eq!(r.debits[&(1, 3)], 20.into());
    assert_eq!(r.debits[&(0, 1)], U256::from(200) * U256::exp10(18));
    assert_eq!(r.fee, CKB);
    assert_eq!(
        r.root,
        state(&f.tx.outputs[0]).account().merkle_root().unpack()
    );
}
#[test]
fn continuation_requires_governance_lock() {
    let mut f = Fixture::new();
    f.tx.inputs[0].lock = f.p.governance.clone();
    f.tx.outpoints[0] = packed::OutPoint::new_builder()
        .tx_hash([42; 32].pack())
        .build();
    f.verify().unwrap();
}
#[test]
fn codec_roundtrip_binds_profile_hash() {
    let f = Fixture::new();
    let pb = encode_profile(&f.p);
    let p = codec::decode_profile(&pb, hash(&pb)).unwrap();
    let a = codec::decode_action(&encode_action(&f.a)).unwrap();
    support::verify_native(&p, &f.cfg, &f.tx, &a).unwrap();
    assert_eq!(
        codec::decode_profile(&pb, [0; 32]).unwrap_err(),
        Error::Profile
    );
    let mut corrupt = pb;
    corrupt[50] ^= 1;
    assert_eq!(
        codec::decode_profile(&corrupt, hash(&encode_profile(&f.p))).unwrap_err(),
        Error::Profile
    );
}
#[test]
fn native_payload_invalid_lengths_and_trailing_bytes_rejected() {
    let f = Fixture::new();
    for tag in [0u32, 1, 2, 3, 5, u32::MAX] {
        let mut b = encode_action(&f.a);
        b[..4].copy_from_slice(&tag.to_le_bytes());
        assert!(codec::decode_action(&b).is_err());
    }
    let mut b = encode_action(&f.a);
    b.push(0);
    assert!(codec::decode_action(&b).is_err());
    for n in [0, 1, 3, 4, 20] {
        assert!(codec::decode_action(&b[..n]).is_err());
    }
}
macro_rules! reject {
    ($name:ident,$error:expr,$body:expr) => {
        #[test]
        fn $name() {
            let mut f = Fixture::new();
            ($body)(&mut f);
            assert_eq!(f.verify().unwrap_err(), $error);
        }
    };
}
reject!(missing_rollup_input, Error::Rollup, |f: &mut Fixture| {
    f.tx.inputs[0].type_ = None;
});
reject!(burn_rollup, Error::Rollup, |f: &mut Fixture| {
    f.tx.outputs[0].type_ = None;
});
reject!(duplicate_rollup_output, Error::Rollup, |f: &mut Fixture| {
    f.tx.outputs.push(f.tx.outputs[0].clone());
});
reject!(
    wrong_bootstrap_outpoint,
    Error::Authority,
    |f: &mut Fixture| {
        f.p.bootstrap_outpoint = packed::OutPoint::default();
    }
);
reject!(
    wrong_input_authority,
    Error::Authority,
    |f: &mut Fixture| {
        f.tx.inputs[0].lock = script([44; 32], vec![]);
    }
);
reject!(
    wrong_output_authority,
    Error::Authority,
    |f: &mut Fixture| {
        f.tx.outputs[0].lock = f.p.bootstrap_lock.clone();
    }
);
reject!(
    rollup_capacity_theft,
    Error::Authority,
    |f: &mut Fixture| {
        f.tx.outputs[0].capacity -= 1;
    }
);
reject!(
    missing_multisig_sponsor,
    Error::Authority,
    |f: &mut Fixture| {
        f.tx.inputs.pop();
        f.tx.outpoints.pop();
        f.tx.outputs.pop();
    }
);
reject!(fake_authority_as_output, Error::Cell, |f: &mut Fixture| {
    f.tx.inputs[3].lock = script([45; 32], vec![]);
});
reject!(wrong_config, Error::Profile, |f: &mut Fixture| {
    f.cfg = f
        .cfg
        .clone()
        .as_builder()
        .finality_blocks(9u64.pack())
        .build();
});
reject!(duplicate_input, Error::Cell, |f: &mut Fixture| {
    f.tx.outpoints[1] = f.tx.outpoints[0].clone();
});
reject!(wrong_recipient_lock, Error::Output, |f: &mut Fixture| {
    f.tx.outputs[3].lock = eth_lock([55; 20]);
});
reject!(omni_admin_flag, Error::Output, |f: &mut Fixture| {
    let mut args = f.tx.outputs[3].lock.args().raw_data().to_vec();
    args[21] = 1;
    f.tx.outputs[3].lock = script(OMNI, args);
});
reject!(omni_args_suffix, Error::Output, |f: &mut Fixture| {
    let mut args = f.tx.outputs[3].lock.args().raw_data().to_vec();
    args.push(0);
    f.tx.outputs[3].lock = script(OMNI, args);
});
reject!(omni_wrong_code, Error::Output, |f: &mut Fixture| {
    f.tx.outputs[3].lock = f.tx.outputs[3]
        .lock
        .clone()
        .as_builder()
        .code_hash([47; 32].pack())
        .build();
});
reject!(omni_data_hash_mode, Error::Output, |f: &mut Fixture| {
    f.tx.outputs[3].lock = f.tx.outputs[3]
        .lock
        .clone()
        .as_builder()
        .hash_type(2u8.into())
        .build();
});
reject!(contract_recipient, Error::Recipient, |f: &mut Fixture| {
    f.a.recipients[0].script = f.a.recipients[0]
        .script
        .clone()
        .as_builder()
        .code_hash([88; 32].pack())
        .build();
});
reject!(
    address_script_mismatch,
    Error::Recipient,
    |f: &mut Fixture| {
        f.a.recipients[0].address = [12; 20];
    }
);
reject!(duplicate_recipient, Error::Encoding, |f: &mut Fixture| {
    f.a.recipients[1] = f.a.recipients[0].clone();
});
reject!(
    duplicate_output_mapping,
    Error::Output,
    |f: &mut Fixture| {
        f.a.payouts[1].output = 3;
    }
);
reject!(
    output_index_out_of_bounds,
    Error::Output,
    |f: &mut Fixture| {
        f.a.payouts[1].output = 999;
    }
);
reject!(hidden_payout_output, Error::Cell, |f: &mut Fixture| {
    f.tx.outputs.push(f.tx.outputs[3].clone());
});
reject!(hidden_input_cell, Error::Cell, |f: &mut Fixture| {
    f.tx.inputs[1].lock = eth_lock([1; 20]);
});
reject!(no_custodian_change, Error::Cell, |f: &mut Fixture| {
    f.tx.outputs[1].lock = script([45; 32], vec![]);
});
reject!(token_burn, Error::Conservation, |f: &mut Fixture| {
    f.tx.outputs[2].data = 169u128.to_le_bytes().to_vec();
});
reject!(token_mint, Error::Conservation, |f: &mut Fixture| {
    f.tx.outputs[2].data = 171u128.to_le_bytes().to_vec();
});
reject!(pool_pays_fee, Error::Conservation, |f: &mut Fixture| {
    f.tx.outputs[1].capacity -= 1;
});
reject!(
    outside_capacity_topup,
    Error::Conservation,
    |f: &mut Fixture| {
        f.tx.outputs[3].capacity += 1;
    }
);
reject!(sponsor_overdrawn, Error::Capacity, |f: &mut Fixture| {
    f.tx.outputs[5].capacity = 101 * CKB;
});
reject!(typed_sponsor, Error::Cell, |f: &mut Fixture| {
    f.tx.inputs[3].type_ = f.tx.inputs[2].type_.clone();
    f.tx.inputs[3].capacity = 400 * CKB;
    f.tx.inputs[3].data = 1u128.to_le_bytes().to_vec();
});
reject!(malformed_token_data, Error::Asset, |f: &mut Fixture| {
    f.tx.outputs[3].data.push(0);
});
reject!(zero_token_payout, Error::Asset, |f: &mut Fixture| {
    f.tx.outputs[3].data = 0u128.to_le_bytes().to_vec();
});
reject!(
    below_occupied_capacity,
    Error::Capacity,
    |f: &mut Fixture| {
        f.tx.outputs[3].capacity = 1;
    }
);
reject!(stale_or_corrupted_proof, Error::Proof, |f: &mut Fixture| {
    f.a.proof.0[0] ^= 0xff;
});
reject!(missing_key, Error::Keys, |f: &mut Fixture| {
    f.a.kv.pop();
});
reject!(extra_key, Error::Keys, |f: &mut Fixture| {
    f.a.kv.push(([0xff; 32], [0; 32]));
});
reject!(duplicate_key, Error::Keys, |f: &mut Fixture| {
    f.a.kv.insert(0, f.a.kv[0]);
});
reject!(wrong_new_root, Error::State, |f: &mut Fixture| {
    set_root(&mut f.tx.outputs[0], [99; 32]);
});
reject!(account_count_change, Error::State, |f: &mut Fixture| {
    let s = state(&f.tx.outputs[0]);
    f.tx.outputs[0].data = s
        .clone()
        .as_builder()
        .account(s.account().as_builder().count(21u32.pack()).build())
        .build()
        .as_slice()
        .to_vec();
});
reject!(finality_metadata_change, Error::State, |f: &mut Fixture| {
    f.tx.outputs[0].data = state(&f.tx.outputs[0])
        .as_builder()
        .last_finalized_timepoint(100u64.pack())
        .build()
        .as_slice()
        .to_vec();
});
reject!(
    insufficient_ckb_for_token_cell,
    Error::Balance,
    |f: &mut Fixture| {
        let k = balance_key(1, f.a.recipients[0].address);
        f.leaves.insert(k, value(U256::from(199) * U256::exp10(18)));
        f.refresh();
    }
);
reject!(
    insufficient_token_balance,
    Error::Balance,
    |f: &mut Fixture| {
        let k = balance_key(3, f.a.recipients[0].address);
        f.leaves.insert(k, value(9.into()));
        f.refresh();
    }
);
reject!(insufficient_supply, Error::Supply, |f: &mut Fixture| {
    f.leaves.insert(supply_key(3), value(29.into()));
    f.refresh();
});
reject!(malformed_custodian, Error::Cell, |f: &mut Fixture| {
    f.tx.inputs[1].lock = script(custodian_hash(), vec![0; 31]);
});
reject!(unfinalized_custodian, Error::Finality, |f: &mut Fixture| {
    let args = f.tx.inputs[1].lock.args().raw_data();
    let c = packed::CustodianLockArgs::from_slice(&args[32..])
        .unwrap()
        .as_builder()
        .deposit_finalized_timepoint(100u64.pack())
        .build();
    f.tx.inputs[1].lock = script(custodian_hash(), [&args[..32], c.as_slice()].concat());
});
reject!(too_many_recipients, Error::Limit, |f: &mut Fixture| {
    f.a.recipients = vec![f.a.recipients[0].clone(); MAX_RECIPIENTS + 1];
});
reject!(empty_batch, Error::Limit, |f: &mut Fixture| {
    f.a.payouts.clear();
});
#[test]
fn tampered_balance_with_valid_shape_fails_authentication() {
    let mut f = Fixture::new();
    let key = balance_key(3, f.a.recipients[0].address);
    f.a.kv.iter_mut().find(|(k, _)| *k == key).unwrap().1 = value(101.into());
    assert_eq!(f.verify().unwrap_err(), Error::Proof);
}
#[test]
fn reverse_mapping_mismatch_is_rejected() {
    let mut f = Fixture::new();
    let sh = script_hash(&f.a.recipients[0].script);
    f.leaves.insert(
        build_account_key(2, &build_script_hash_to_registry_address_key(&sh)),
        [0; 32],
    );
    f.refresh();
    assert_eq!(f.verify().unwrap_err(), Error::Recipient);
}
#[test]
fn wire_resource_limits_before_payload_allocation() {
    assert_eq!(
        codec::decode_action(&vec![0; MAX_ACTION + 1]).unwrap_err(),
        Error::Limit
    );
    assert_eq!(
        codec::decode_profile(&vec![0; MAX_PROFILE + 1], [1; 32]).unwrap_err(),
        Error::Limit
    );
}
#[test]
fn hidden_storage_mutation_cannot_change_output_root() {
    let mut f = Fixture::new();
    let mut tree = f.tree();
    tree.update([0xee; 32].into(), [1; 32].into()).unwrap();
    set_root(&mut f.tx.outputs[0], (*tree.root()).into());
    assert_eq!(f.verify().unwrap_err(), Error::State);
}
#[test]
fn native_only_partial_exit() {
    let mut f = Fixture::new();
    for i in [3, 4] {
        f.tx.outputs[i].type_ = None;
        f.tx.outputs[i].data.clear();
    }
    f.tx.outputs[2].data = 200u128.to_le_bytes().to_vec();
    for p in &mut f.a.payouts {
        p.asset = 1;
    }
    f.a.assets.pop();
    f.keys.remove(&supply_key(3));
    f.keys
        .remove(&build_account_field_key(3, GW_ACCOUNT_SCRIPT_HASH_TYPE));
    for r in &f.a.recipients {
        f.keys.remove(&balance_key(3, r.address));
    }
    f.refresh();
    f.verify().unwrap();
}
#[test]
fn token_conservation_does_not_wrap_u128() {
    let mut f = Fixture::new();
    f.tx.inputs[2].data = u128::MAX.to_le_bytes().to_vec();
    f.tx.outputs[2].data = (u128::MAX - 30).to_le_bytes().to_vec();
    f.verify().unwrap();
    f.tx.outputs[2].data = u128::MAX.to_le_bytes().to_vec();
    assert_eq!(f.verify().unwrap_err(), Error::Conservation);
}
#[test]
fn omitted_absent_balance_is_not_zero_by_default() {
    let mut f = Fixture::new();
    let k = balance_key(3, f.a.recipients[0].address);
    f.leaves.remove(&k);
    f.refresh();
    assert_eq!(f.verify().unwrap_err(), Error::Balance);
    f.a.kv.retain(|(key, _)| *key != k);
    assert_eq!(f.verify().unwrap_err(), Error::Keys);
}
#[test]
fn consecutive_batches_debit_current_state() {
    let mut f = Fixture::new();
    let initial = f.verify().unwrap().root;
    f.advance();
    assert_eq!(
        state(&f.tx.inputs[0]).account().merkle_root().unpack(),
        initial
    );
    let second = f.verify().unwrap();
    assert_ne!(second.root, initial);
    assert_eq!(
        amount(f.leaves[&balance_key(3, f.a.recipients[0].address)]),
        90.into()
    );
}
#[test]
fn replay_old_proof_against_updated_state_fails() {
    let mut f = Fixture::new();
    let old = f.a.clone();
    f.advance();
    f.a = old;
    assert_eq!(f.verify().unwrap_err(), Error::Proof);
}
#[test]
fn replay_bootstrap_on_new_outpoint_fails() {
    let mut f = Fixture::new();
    f.advance();
    f.tx.inputs[0].lock = f.p.bootstrap_lock.clone();
    assert_eq!(f.verify().unwrap_err(), Error::Authority);
}
#[test]
fn token_balance_can_reach_zero() {
    let mut f = Fixture::new();
    f.leaves
        .insert(balance_key(3, f.a.recipients[0].address), value(10.into()));
    f.refresh();
    f.verify().unwrap();
}
#[test]
fn supply_only_output_root_is_rejected() {
    let mut f = Fixture::new();
    let mut tree = f.tree();
    tree.update(supply_key(3).into(), value(170.into()).into())
        .unwrap();
    set_root(&mut f.tx.outputs[0], (*tree.root()).into());
    assert_eq!(f.verify().unwrap_err(), Error::State);
}
#[test]
fn malformed_proof_suffix_is_rejected() {
    let mut f = Fixture::new();
    f.a.proof.0.push(0xff);
    assert_eq!(f.verify().unwrap_err(), Error::Proof);
}
#[test]
fn unregistered_asset_substitution_is_rejected() {
    let mut f = Fixture::new();
    f.a.assets[1].id = 4;
    assert_eq!(f.verify().unwrap_err(), Error::Asset);
}
#[test]
fn custodian_timestamp_finality_boundary() {
    use gw_types::core::Timepoint;
    let mut f = Fixture::new();
    let gs = state(&f.tx.inputs[0])
        .as_builder()
        .last_finalized_timepoint(Timepoint::Timestamp(500).full_value().pack())
        .build();
    f.tx.inputs[0].data = gs.as_slice().to_vec();
    f.tx.outputs[0].data = gs.as_slice().to_vec();
    for (time, expected) in [(499, true), (500, true), (501, false)] {
        let args = f.tx.inputs[1].lock.args().raw_data();
        let ca = packed::CustodianLockArgs::from_slice(&args[32..])
            .unwrap()
            .as_builder()
            .deposit_finalized_timepoint(Timepoint::Timestamp(time).full_value().pack())
            .build();
        f.tx.inputs[1].lock = script(custodian_hash(), [&args[..32], ca.as_slice()].concat());
        f.refresh();
        assert_eq!(f.verify().is_ok(), expected);
    }
}
#[test]
fn sponsor_capacity_overflow_is_rejected() {
    let mut f = Fixture::new();
    f.tx.inputs[3].capacity = u64::MAX;
    f.tx.inputs
        .push(cell(100, f.p.governance.clone(), None, vec![]));
    f.tx.outpoints.push(
        packed::OutPoint::new_builder()
            .tx_hash([70; 32].pack())
            .build(),
    );
    assert_eq!(f.verify().unwrap_err(), Error::Overflow);
}
reject!(proof_size_limit, Error::Limit, |f: &mut Fixture| {
    f.a.proof.0 = vec![0; MAX_PROOF + 1];
});
reject!(cell_data_size_limit, Error::Limit, |f: &mut Fixture| {
    f.tx.inputs[1].data = vec![0; MAX_CELL_DATA + 1];
});
reject!(script_size_limit, Error::Limit, |f: &mut Fixture| {
    f.tx.inputs[1].lock = script(custodian_hash(), vec![0; MAX_SCRIPT]);
});
reject!(unknown_payout_asset, Error::Asset, |f: &mut Fixture| {
    f.a.payouts[1].asset = 40;
});
reject!(wrong_sudt_type, Error::Asset, |f: &mut Fixture| {
    f.tx.outputs[3].type_ = Some(script([11; 32], vec![78; 32]));
});
reject!(freeze_status_change, Error::State, |f: &mut Fixture| {
    f.tx.outputs[0].data = state(&f.tx.outputs[0])
        .as_builder()
        .status(1u8.into())
        .build()
        .as_slice()
        .to_vec();
});
#[test]
fn eight_recipient_batch() {
    let mut f = Fixture::new();
    f.eight_recipients();
    assert_eq!(f.verify().unwrap().debits.len(), 16);
}
#[test]
fn split_same_asset_payouts() {
    let mut f = Fixture::new();
    let mut split = f.tx.outputs[3].clone();
    split.data = 5u128.to_le_bytes().to_vec();
    f.tx.outputs[3].data = 5u128.to_le_bytes().to_vec();
    f.tx.outputs[1].capacity -= split.capacity;
    f.tx.outputs.push(split);
    f.a.payouts.insert(
        1,
        Payout {
            output: 6,
            recipient: 0,
            asset: 3,
        },
    );
    f.refresh();
    let r = f.verify().unwrap();
    assert_eq!(r.debits[&(0, 3)], 10.into());
    assert_eq!(r.debits[&(0, 1)], U256::from(400) * U256::exp10(18));
}
#[test]
fn exhausted_token_balance_cannot_pay_again() {
    let mut f = Fixture::new();
    f.leaves
        .insert(balance_key(3, f.a.recipients[0].address), value(10.into()));
    f.refresh();
    f.advance();
    assert_eq!(f.verify().unwrap_err(), Error::Balance);
}

#[test]
fn explicit_adversarial_matrix() {
    for (name, f, expected) in adversarial::cases() {
        assert_eq!(f.verify().unwrap_err(), expected, "{name}");
    }
}

#[test]
fn extra_custodians_are_allowed_only_when_conserved() {
    let f = adversarial::balanced_extra_custodian();
    assert_eq!(f.verify().unwrap(), Fixture::new().verify().unwrap());
}

#[test]
fn borrowed_global_state_comparison_preserves_all_other_fields() {
    let f = Fixture::new();
    let s = state(&f.tx.outputs[0]);
    let mutations = [
        s.clone()
            .as_builder()
            .rollup_config_hash([71; 32].pack())
            .build(),
        s.clone()
            .as_builder()
            .block(s.block().as_builder().count(101u64.pack()).build())
            .build(),
        s.clone()
            .as_builder()
            .reverted_block_root([72; 32].pack())
            .build(),
        s.clone()
            .as_builder()
            .tip_block_hash([73; 32].pack())
            .build(),
        s.clone()
            .as_builder()
            .tip_block_timestamp(1u64.pack())
            .build(),
        s.clone()
            .as_builder()
            .last_finalized_timepoint(1u64.pack())
            .build(),
        s.clone().as_builder().status(1u8.into()).build(),
        s.clone().as_builder().version(2u8.into()).build(),
        s.clone()
            .as_builder()
            .account(s.account().as_builder().count(101u32.pack()).build())
            .build(),
    ];
    for changed in mutations {
        let mut f = Fixture::new();
        f.tx.outputs[0].data = changed.as_slice().to_vec();
        assert_eq!(f.verify().unwrap_err(), Error::State);
    }
}

#[test]
fn v0_id_reverse_mapping_is_plain_le32_not_v1_exists_flag() {
    use native::{short_key, NativeVersion};
    for id in [0u32, 1, 24402] {
        let sh = [0x42; 32];
        let short = [0x42; 20];
        let mut leaves = std::collections::BTreeMap::from([
            (build_account_field_key(id, GW_ACCOUNT_SCRIPT_HASH_TYPE), sh),
            (short_key(short), sh),
        ]);
        let key = build_script_hash_to_account_id_key(&sh);
        let mut raw = [0; 32];
        raw[..4].copy_from_slice(&id.to_le_bytes());
        leaves.insert(key, raw);
        assert!(NativeVersion::V0
            .identity(id, sh, short, 50000, |k| Ok(*leaves
                .get(&k)
                .unwrap_or(&[0; 32])))
            .is_ok());
        raw[4] = 1;
        leaves.insert(key, raw);
        assert_eq!(
            NativeVersion::V0
                .identity(id, sh, short, 50000, |k| Ok(*leaves
                    .get(&k)
                    .unwrap_or(&[0; 32])))
                .unwrap_err(),
            Error::Recipient
        );
    }
}
