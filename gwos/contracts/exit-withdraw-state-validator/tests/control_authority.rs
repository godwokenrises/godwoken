use exit_withdraw_state_validator::{self as c, control::*};
use gw_types::{packed, prelude::*};
use std::collections::BTreeSet;
fn cell(owner: [u8; 20]) -> c::Cell {
    c::Cell {
        capacity: 70_00000000,
        lock: c::eth_lock(owner),
        type_: None,
        data: vec![],
    }
}
#[test]
fn thresholds_and_exact_authorization_cell_return() {
    for (n, m) in [(2, 3), (2, 4), (3, 6)] {
        let owners = (1..=m).map(|i| [i; 20]).collect::<Vec<_>>();
        let script = descriptor([9; 32], 123, n, &owners).unwrap();
        let a = c::Action {
            recipients: vec![c::Recipient {
                id: 123,
                address: identity(&script),
                script,
            }],
            assets: vec![],
            payouts: vec![c::Payout {
                output: 0,
                recipient: 0,
                asset: 1,
            }],
            kv: vec![],
            proof: gw_smt::smt::CompiledMerkleProof(vec![]),
        };
        let inputs = owners[..n as usize]
            .iter()
            .map(|o| cell(*o))
            .collect::<Vec<_>>();
        let mut outputs = vec![cell([55; 20])];
        outputs.extend(inputs.clone());
        let tx = c::Transaction {
            inputs,
            outputs,
            outpoints: vec![],
        };
        let used = BTreeSet::from([0]);
        assert!(authorize(&tx, &a, &used, [9; 32], true).is_ok());
        let mut bad = tx.clone();
        bad.inputs.pop();
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
        let mut bad = tx.clone();
        bad.inputs[1] = bad.inputs[0].clone();
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
        let mut bad = tx.clone();
        bad.inputs[1] = cell([88; 20]);
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
        let mut bad = tx.clone();
        bad.outputs[1].capacity -= 1;
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
        let mut bad = tx.clone();
        bad.outputs.remove(1);
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
        let mut bad = a.clone();
        bad.recipients[0].address = [8; 20];
        assert!(authorize(&tx, &bad, &used, [9; 32], true).is_err());
        assert!(authorize(&tx, &a, &used, [8; 32], true).is_err());
        let mut bad = tx.clone();
        let mut args = bad.inputs[0].lock.args().raw_data().to_vec();
        args.push(0);
        bad.inputs[0].lock = bad.inputs[0]
            .lock
            .clone()
            .as_builder()
            .args(args.pack())
            .build();
        assert!(authorize(&bad, &a, &used, [9; 32], true).is_err());
    }
}
#[test]
fn descriptors_reject_duplicate_unsorted_owners_invalid_threshold_and_trailing_args() {
    assert!(descriptor([1; 32], 1, 2, &[[1; 20], [1; 20]]).is_err());
    assert!(descriptor([1; 32], 1, 2, &[[2; 20], [1; 20]]).is_err());
    assert!(descriptor([1; 32], 1, 0, &[[1; 20]]).is_err());
    assert!(descriptor([1; 32], 1, 2, &[[1; 20]]).is_err());
    assert!(descriptor([1; 32], 1, 1, &[[0; 20]]).is_err());
    let d = descriptor([1; 32], 1, 1, &[[1; 20]]).unwrap();
    let mut args = d.args().raw_data().to_vec();
    args.push(0);
    let d: packed::Script = d.as_builder().args(args.pack()).build();
    assert!(validate(&d, [1; 32], 1).is_err());
}
