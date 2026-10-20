//! Explicit user-requested negative cases, shared by host and real VM tests.
use super::*;

pub fn cases() -> Vec<(&'static str, Fixture, Error)> {
    let mut cases = Vec::new();
    macro_rules! case {
        ($name:literal, $error:expr, $mutate:expr) => {{
            let mut f = Fixture::new();
            f.verify().unwrap();
            ($mutate)(&mut f);
            cases.push(($name, f, $error));
        }};
    }
    case!(
        "recipient_address_disagrees_with_proven_account",
        Error::Recipient,
        |f: &mut Fixture| {
            f.a.recipients[0].address = [12; 20];
        }
    );
    case!(
        "payout_address_disagrees_with_recipient",
        Error::Output,
        |f: &mut Fixture| {
            f.tx.outputs[3].lock = eth_lock(f.a.recipients[1].address);
        }
    );
    case!(
        "payout_amount_breaks_pool_conservation",
        Error::Conservation,
        |f: &mut Fixture| {
            f.tx.outputs[3].data = 11u128.to_le_bytes().to_vec();
        }
    );
    case!(
        "conserved_payout_amount_disagrees_with_l2_debit",
        Error::State,
        |f: &mut Fixture| {
            // Keep the original output root (10 debited), but pay 11 and return one less.
            f.tx.outputs[3].data = 11u128.to_le_bytes().to_vec();
            f.tx.outputs[2].data = 169u128.to_le_bytes().to_vec();
        }
    );
    case!(
        "duplicate_withdraw_address",
        Error::Encoding,
        |f: &mut Fixture| {
            f.a.recipients.insert(1, f.a.recipients[0].clone());
        }
    );
    case!(
        "listed_address_without_output_cell",
        Error::Output,
        |f: &mut Fixture| {
            // Remove the second recipient's cell and mapping; keep their entry in the address list.
            f.a.payouts.pop();
            f.tx.outputs.remove(4);
            f.tx.outputs[1].capacity += 200 * CKB;
            f.tx.outputs[2].data = 190u128.to_le_bytes().to_vec();
        }
    );
    case!(
        "listed_address_without_payout_mapping",
        Error::Output,
        |f: &mut Fixture| {
            f.a.payouts.pop();
        }
    );
    case!(
        "payout_lock_args_suffix",
        Error::Output,
        |f: &mut Fixture| {
            let mut args = f.tx.outputs[3].lock.args().raw_data().to_vec();
            args.extend_from_slice(&[0, 1]);
            f.tx.outputs[3].lock = script(OMNI, args);
        }
    );
    case!(
        "payout_sudt_type_args_mismatch",
        Error::Asset,
        |f: &mut Fixture| {
            f.tx.outputs[3].type_ = Some(script([11; 32], vec![78; 32]));
        }
    );
    case!(
        "payout_sudt_code_mismatch",
        Error::Asset,
        |f: &mut Fixture| {
            f.tx.outputs[3].type_ = Some(script([15; 32], vec![77; 32]));
        }
    );
    case!(
        "extra_custodian_input_not_returned",
        Error::Conservation,
        |f: &mut Fixture| {
            f.tx.inputs.push(f.tx.inputs[1].clone());
            f.tx.outpoints.push(
                packed::OutPoint::new_builder()
                    .tx_hash([90; 32].pack())
                    .build(),
            );
        }
    );
    case!(
        "extra_custodian_output_not_funded",
        Error::Conservation,
        |f: &mut Fixture| {
            f.tx.outputs.push(f.tx.outputs[1].clone());
        }
    );
    case!(
        "foreign_rollup_custodian_input",
        Error::Cell,
        |f: &mut Fixture| {
            let mut args = f.tx.inputs[1].lock.args().raw_data().to_vec();
            args[0] ^= 1;
            f.tx.inputs[1].lock = script(custodian_hash(), args);
        }
    );
    case!(
        "noncanonical_custodian_output",
        Error::Cell,
        |f: &mut Fixture| {
            let args = f.tx.outputs[1].lock.args().raw_data();
            let ca = packed::CustodianLockArgs::from_slice(&args[32..])
                .unwrap()
                .as_builder()
                .deposit_block_hash([1; 32].pack())
                .build();
            f.tx.outputs[1].lock = script(custodian_hash(), [&args[..32], ca.as_slice()].concat());
        }
    );
    case!(
        "unlisted_eth_address_in_extra_output",
        Error::Cell,
        |f: &mut Fixture| {
            // Fund the hidden cell from custodian change so conservation alone cannot detect it.
            let mut extra = f.tx.outputs[3].clone();
            extra.lock = eth_lock([55; 20]);
            extra.data = 1u128.to_le_bytes().to_vec();
            f.tx.outputs[1].capacity -= extra.capacity;
            f.tx.outputs[2].data = 169u128.to_le_bytes().to_vec();
            f.tx.outputs.push(extra);
        }
    );
    case!(
        "listed_eth_address_in_unmapped_extra_output",
        Error::Cell,
        |f: &mut Fixture| {
            let mut extra = f.tx.outputs[3].clone();
            extra.data = 1u128.to_le_bytes().to_vec();
            f.tx.outputs[1].capacity -= extra.capacity;
            f.tx.outputs[2].data = 169u128.to_le_bytes().to_vec();
            f.tx.outputs.push(extra);
        }
    );
    case!(
        "duplicate_payout_cell_reference",
        Error::Encoding,
        |f: &mut Fixture| {
            f.a.payouts.insert(
                1,
                Payout {
                    output: 3,
                    recipient: 0,
                    asset: 3,
                },
            );
        }
    );
    cases
}

pub fn balanced_extra_custodian() -> Fixture {
    let mut f = Fixture::new();
    let extra = f.tx.inputs[1].clone();
    f.tx.inputs.push(extra.clone());
    f.tx.outpoints.push(
        packed::OutPoint::new_builder()
            .tx_hash([90; 32].pack())
            .build(),
    );
    f.tx.outputs.push(extra);
    f
}
