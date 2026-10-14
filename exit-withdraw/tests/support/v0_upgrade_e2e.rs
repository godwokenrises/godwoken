//! Isolated deployed PoA/state-lock compatibility, not a v0 settlement validator.
//! Production setup layout and TYPE_IDs; identities replaced with public test keys.
use super::*;
pub(super) fn evidence(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("tests/fixtures/mainnet-evidence/v0-upgrade/{name}.json")).unwrap())
        .unwrap()
}
pub(super) fn poa_fixture(consensus: bool, owner_count: usize) -> (Node, Transaction) {
    let (mut node, mut deps, _, _, _) = setup_claim();
    for name in ["state_validator_lock", "poa_state"] {
        let v = evidence(&format!("{name}-recorded-live"));
        let cell = &v["response"]["result"]["cell"];
        let ty: Script = serde_json::from_value(cell["output"]["type"].clone()).unwrap();
        let data = cell["data"]["content"].as_str().unwrap().to_owned();
        assert_eq!(hx(hash(&bytes(&data).unwrap())), cell["data"]["hash"]);
        let expected = if name == "state_validator_lock" {
            "0xede199745fbd9e9f339f1dd2e92a586020d111432bfb81a6c2b12ddc9856f0ba"
        } else {
            "0x1c04df09d9adede5bfc40ff1a39a3a17fc8e29f15c56f16b7e48680c600ee5ac"
        };
        assert_eq!(ty.hash().unwrap(), expected);
        let tx = base_tx(
            vec![CellOutput {
                capacity: format!("0x{:x}", 1_000_000 * CAP),
                lock: script([0; 32], vec![]),
                type_script: Some(ty),
            }],
            vec![data.clone()],
        );
        deps.push(Dependency {
            out_point: point(&tx, 0),
            dep_type: "code".into(),
            data_hash: hx(hash(&bytes(&data).unwrap())),
        });
        node.insert(tx);
    }
    let roll = evidence("rollup-current");
    let poa: Script =
        serde_json::from_value(roll["response"]["result"]["objects"][0]["output"]["lock"].clone())
            .unwrap();
    let setup = evidence("poa-setup-current");
    let data = evidence("poa-data-current");
    let setup_cell = &setup["response"]["result"]["objects"][0];
    let data_cell = &data["response"]["result"]["objects"][0];
    let setup_output: CellOutput = serde_json::from_value(setup_cell["output"].clone()).unwrap();
    let data_output: CellOutput = serde_json::from_value(data_cell["output"].clone()).unwrap();
    let owners = [
        claim::owner(&hx(eth(1))).unwrap(),
        claim::owner(&hx(eth(2))).unwrap(),
    ];
    let mut setup_bytes = bytes(setup_cell["output_data"].as_str().unwrap()).unwrap();
    assert_eq!(&setup_bytes[..12], &[1, 32, 2, 2, 24, 0, 0, 0, 1, 0, 0, 0]);
    for (i, owner) in owners.iter().enumerate() {
        setup_bytes[12 + i * 32..44 + i * 32]
            .copy_from_slice(&hash32(&owner.hash().unwrap()).unwrap());
    }
    let old_data = bytes(data_cell["output_data"].as_str().unwrap()).unwrap();
    let last = u64::from_le_bytes(old_data[..8].try_into().unwrap());
    let now = last + 48; // Producer 0 is eligible after both 24-second rounds.
    let mut next_data = old_data.clone();
    next_data[..8].copy_from_slice(&now.to_le_bytes());
    next_data[8..16].copy_from_slice(&now.to_le_bytes());
    next_data[16..22].fill(0);
    let mut funding = base_tx(
        vec![
            CellOutput {
                capacity: format!("0x{:x}", 500 * CAP),
                lock: poa,
                type_script: None,
            },
            setup_output.clone(),
            data_output.clone(),
        ],
        vec!["0x".into(), hx(&setup_bytes), hx(&old_data)],
    );
    for owner in &owners {
        funding.outputs.push(CellOutput {
            capacity: format!("0x{:x}", 500 * CAP),
            lock: owner.clone(),
            type_script: None,
        });
        funding.outputs_data.push("0x".into());
    }
    let funding_points: Vec<_> = (0..5).map(|i| point(&funding, i)).collect();
    node.insert(funding);
    let mut tx = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 500 * CAP),
            lock: owners[0].clone(),
            type_script: None,
        }],
        vec!["0x".into()],
    );
    tx.cell_deps = deps
        .iter()
        .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
        .collect();
    tx.inputs.push(json!({"previous_output":funding_points[0],"since":format!("0x{:x}",0x4000_0000_0000_0000u64|now)}));
    if consensus {
        tx.inputs
            .push(json!({"previous_output":funding_points[1],"since":"0x0"}));
        tx.outputs.push(setup_output);
        tx.outputs_data.push(hx(&setup_bytes));
    } else {
        tx.cell_deps
            .push(json!({"out_point":funding_points[1],"dep_type":"code"}));
        tx.inputs
            .push(json!({"previous_output":funding_points[2],"since":"0x0"}));
        tx.outputs.push(data_output);
        tx.outputs_data.push(hx(next_data));
    }
    tx.witnesses = vec!["0x".into(); 2];
    for i in 0..owner_count {
        tx.inputs
            .push(json!({"previous_output":funding_points[3+i],"since":"0x0"}));
        tx.outputs.push(CellOutput {
            capacity: format!("0x{:x}", 500 * CAP),
            lock: owners[i].clone(),
            type_script: None,
        });
        tx.outputs_data.push("0x".into());
        tx.witnesses.push(hx(packed::WitnessArgs::new_builder()
            .lock(Some(gw_types::bytes::Bytes::from(omni_signature(&[0; 65]))).pack())
            .build()
            .as_slice()));
    }
    for i in 0..owner_count {
        sign_group(&mut tx, &[2 + i], (i + 1) as u8, true);
    }
    (node, tx)
}
#[test]
fn v0_deployed_poa_normal_path_can_change_output_lock() {
    let (node, tx) = poa_fixture(false, 1);
    println!(
        "V0 PoA normal lock transition: {} cycles",
        node.vm(&tx).unwrap()
    );
    let (node, tx) = poa_fixture(false, 0);
    let e = node.vm(&tx).unwrap_err().to_string();
    assert!(e.contains("Inputs[0].Lock"), "{e}");
    println!("V0 PoA missing producer rejected: {e}");
    let (node, mut tx) = poa_fixture(false, 1);
    let mut b = bytes(&tx.outputs_data[1]).unwrap();
    b[8] ^= 1;
    tx.outputs_data[1] = hx(b);
    sign_group(&mut tx, &[2], 1, true);
    let e = node.vm(&tx).unwrap_err().to_string();
    assert!(e.contains("Inputs[0].Lock"), "{e}");
    println!("V0 PoA inconsistent timestamp rejected: {e}");
}
#[test]
fn v0_deployed_poa_consensus_requires_two_distinct_identities() {
    let (node, tx) = poa_fixture(true, 2);
    println!(
        "V0 PoA consensus lock transition: {} cycles",
        node.vm(&tx).unwrap()
    );
    let (node, tx) = poa_fixture(true, 1);
    let e = node.vm(&tx).unwrap_err().to_string();
    assert!(e.contains("Inputs[0].Lock"), "{e}");
    println!("V0 PoA one of two rejected: {e}");
}
#[test]
fn v0_historical_upgrade_reveals_current_three_of_five_config() {
    let upgraded = evidence("validator-upgrade-transaction");
    let previous = evidence("validator-upgrade-previous");
    let live = evidence("state_validator-recorded-live");
    let tx = &upgraded["response"]["result"]["transaction"];
    let old = &previous["response"]["result"]["transaction"]["outputs"][0];
    assert_eq!(
        tx["inputs"][0]["previous_output"]["tx_hash"],
        "0x5a4b33c82a87473adfba22d0129d45355569fd47111610a3fde4008f43f03f8e"
    );
    assert_eq!(tx["inputs"][0]["previous_output"]["index"], "0x0");
    assert_eq!(
        old["lock"],
        live["response"]["result"]["cell"]["output"]["lock"]
    );
    assert_eq!(old["type"], tx["outputs"][0]["type"]);
    let w = packed::WitnessArgs::from_slice(&bytes(tx["witnesses"][0].as_str().unwrap()).unwrap())
        .unwrap();
    let lock = w.lock().to_opt().unwrap().raw_data();
    assert_eq!(&lock[..4], &[0, 0, 3, 5]);
    assert_eq!(lock.len(), 104 + 3 * 65);
    assert_eq!(hx(&hash(&lock[..104])[..20]), old["lock"]["args"]);
}

#[test]
fn v0_deployed_poa_consensus_can_retire_both_helper_locks() {
    let (node, mut tx) = poa_fixture(true, 2);
    let v = evidence("poa-data-current");
    let data_type: Script =
        serde_json::from_value(v["response"]["result"]["objects"][0]["output"]["type"].clone())
            .unwrap();
    let data_cell = node
        .live
        .values()
        .find(|c| c.output.type_script.as_ref() == Some(&data_type))
        .unwrap();
    tx.inputs
        .push(json!({"previous_output":data_cell.out_point,"since":"0x0"}));
    tx.witnesses.push("0x".into());
    let destination = tx.outputs[0].lock.clone();
    tx.outputs[1].lock = destination.clone();
    tx.outputs.push(CellOutput {
        lock: destination,
        ..data_cell.output.clone()
    });
    tx.outputs_data.push(data_cell.output_data.clone());
    sign_group(&mut tx, &[2], 1, true);
    sign_group(&mut tx, &[3], 2, true);
    println!(
        "V0 PoA consensus with both helper locks retired: {} cycles",
        node.vm(&tx).unwrap()
    );
}

#[test]
fn v0_live_poa_references_are_type_id_args_not_script_hashes() {
    let v = evidence("rollup-current");
    let lock: Script =
        serde_json::from_value(v["response"]["result"]["objects"][0]["output"]["lock"].clone())
            .unwrap();
    let args = bytes(&lock.args).unwrap();
    assert_eq!(args.len(), 64);
    for (i, name) in ["poa-setup-current", "poa-data-current"].iter().enumerate() {
        let e = evidence(name);
        let cell = &e["response"]["result"]["objects"][0];
        let ty: Script = serde_json::from_value(cell["output"]["type"].clone()).unwrap();
        assert_eq!(
            ty.code_hash,
            "0x00000000000000000000000000000000000000000000000000545950455f4944"
        );
        assert_eq!(ty.hash_type, "type");
        assert_eq!(bytes(&ty.args).unwrap(), args[i * 32..(i + 1) * 32]);
        assert_ne!(ty.hash().unwrap(), ty.args);
        assert_eq!(cell["output"]["lock"]["args"], lock.hash().unwrap());
    }
}
