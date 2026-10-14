//! V0 identity research only. This does not enable v0 settlement or the v1 portal.
use super::*;
const ROLLUP: &str = "0x40d73f0d3c561fcaae330eabc030d8d96a9d0af36d0c5114883658a350cb9e3b";
const ETH_LOCK: &str = "0x1563080d175bf8ddd44a48e850cecf0c0b4575835756eb5ffd53ad830931b9f9";
const TRON_LOCK: &str = "0x0bc55f318d738bd1f50e5d950110f0ce5ea2230ec1b62defe1271e06e680476f";
fn eoa(owner: &[u8; 20]) -> Script {
    Script {
        code_hash: ETH_LOCK.into(),
        hash_type: "type".into(),
        args: hx([bytes(ROLLUP).unwrap(), owner.to_vec()].concat()),
    }
}
// Research-only strict mapping; production must first authenticate this script against the v0 state root.
fn owner_from_script(s: &Script) -> Result<[u8; 20]> {
    anyhow::ensure!(
        s.code_hash == ETH_LOCK && s.hash_type == "type",
        "Not the v0 Ethereum account lock"
    );
    let args = bytes(&s.args)?;
    anyhow::ensure!(
        args.len() == 52 && args[..32] == bytes(ROLLUP)?,
        "Wrong v0 rollup or argument length"
    );
    Ok(args[32..].try_into().unwrap())
}
#[test]
fn v0_live_applicant_short_address_maps_to_distinct_eth_owner() {
    let v: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/mainnet-evidence/v0-identity/account-24402-script.json"
    ))
    .unwrap();
    let script: Script = serde_json::from_value(v["response"]["result"].clone()).unwrap();
    assert_eq!(
        script.hash().unwrap(),
        "0xdeb3312e58616e345acf9bef412a8c13d9ab4fb4e2ac64d6d93eb19702353da8"
    );
    let short = script.hash().unwrap()[..42].to_owned();
    let owner = owner_from_script(&script).unwrap();
    assert_eq!(hx(owner), "0xf9229a907fbf826519a50df021e023ea4f8f314f");
    assert_ne!(short, hx(owner));
    assert_eq!(
        claim::owner(&hx(owner)).unwrap().args,
        "0x01f9229a907fbf826519a50df021e023ea4f8f314f00"
    );
    let mut wrong = script.clone();
    wrong.code_hash = TRON_LOCK.into();
    assert!(owner_from_script(&wrong).is_err());
    let mut wrong = script.clone();
    wrong.hash_type = "data".into();
    assert!(owner_from_script(&wrong).is_err());
    let mut wrong = script.clone();
    wrong.args.push_str("00");
    assert!(owner_from_script(&wrong).is_err());
    let mut wrong = script;
    wrong.args.replace_range(2..66, &"00".repeat(32));
    assert!(owner_from_script(&wrong).is_err());
}
#[test]
fn v0_eth_owner_can_unlock_standard_omnilock_but_short_address_cannot() {
    let (mut node, deps, inputs, assets, mut r) = setup_claim();
    let script = eoa(&eth(1));
    let owner = owner_from_script(&script).unwrap();
    let short = script.hash().unwrap()[..42].to_owned();
    assert_ne!(hx(owner), short);
    r.address = hx(owner);
    let q = claim::construct(&r, inputs.clone(), &assets, &deps, 1000).unwrap();
    let tx = claim::assemble(&q, &signature(&q.signing_message, 1)).unwrap();
    println!(
        "V0 owner / standard OmniLock: {} cycles",
        node.vm(&tx).unwrap()
    );
    // Create a bad payout at the short address, then sign with the same actual owner's key.
    let wrong_lock = claim::owner(&short).unwrap();
    let wrong_funding = base_tx(
        inputs
            .iter()
            .map(|c| CellOutput {
                lock: wrong_lock.clone(),
                ..c.output.clone()
            })
            .collect(),
        inputs.iter().map(|c| c.output_data.clone()).collect(),
    );
    let wrong_inputs: Vec<_> = (0..3)
        .map(|i| wrong_funding.cell(&point(&wrong_funding, i)).unwrap())
        .collect();
    node.insert(wrong_funding);
    r.address = short;
    r.inputs = wrong_inputs.iter().map(|c| c.out_point.clone()).collect();
    let wrong = claim::construct(&r, wrong_inputs, &assets, &deps, 1000).unwrap();
    assert!(claim::assemble(&wrong, &signature(&wrong.signing_message, 1)).is_err());
    let mut forged = wrong.transaction;
    sign_group(&mut forged, &[0, 1, 2], 1, true);
    let rejection = node.vm(&forged).unwrap_err().to_string();
    assert!(
        rejection.contains("Inputs[0].Lock") && rejection.contains("error code -31"),
        "{rejection}"
    );
    println!("Wrong v0 short-address OmniLock owner: {rejection}");
}
#[test]
fn v0_deployed_eth_lock_accepts_same_personal_sign_key_and_rejects_tron_prefix() {
    let (mut node, mut deps, _, _, r) = setup_claim();
    let evidence: Value = serde_json::from_slice(include_bytes!(
        "../fixtures/mainnet-evidence/v0-identity/eth-account-lock-live.json"
    ))
    .unwrap();
    let cell = &evidence["response"]["result"]["cell"];
    let type_script: Script = serde_json::from_value(cell["output"]["type"].clone()).unwrap();
    assert_eq!(type_script.hash().unwrap(), ETH_LOCK);
    let data = cell["data"]["content"].as_str().unwrap().to_owned();
    assert_eq!(
        hx(hash(&bytes(&data).unwrap())),
        cell["data"]["hash"].as_str().unwrap()
    );
    let code = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 1000000 * CAP),
            lock: script([0; 32], vec![]),
            type_script: Some(type_script),
        }],
        vec![data.clone()],
    );
    deps.push(Dependency {
        out_point: point(&code, 0),
        dep_type: "code".into(),
        data_hash: hx(hash(&bytes(&data).unwrap())),
    });
    node.insert(code);
    let owner = claim::owner(&hx(eth(1))).unwrap();
    let message = [7u8; 32];
    let input_data = hx([bytes(&owner.hash().unwrap()).unwrap(), message.to_vec()].concat());
    let funding = base_tx(
        vec![
            CellOutput {
                capacity: format!("0x{:x}", 500 * CAP),
                lock: eoa(&eth(1)),
                type_script: None,
            },
            CellOutput {
                capacity: format!("0x{:x}", 500 * CAP),
                lock: owner,
                type_script: None,
            },
        ],
        vec![input_data, "0x".into()],
    );
    let points: Vec<_> = (0..2).map(|i| point(&funding, i)).collect();
    node.insert(funding);
    let mut tx = base_tx(
        vec![CellOutput {
            capacity: format!("0x{:x}", 999 * CAP),
            lock: claim::parse_address(&r.destination).unwrap(),
            type_script: None,
        }],
        vec!["0x".into()],
    );
    tx.inputs = points
        .iter()
        .map(|p| json!({"since":"0x0","previous_output":p}))
        .collect();
    tx.cell_deps = deps
        .iter()
        .map(|d| json!({"out_point":d.out_point,"dep_type":d.dep_type}))
        .collect();
    let witness = |signature: Vec<u8>| {
        hx(packed::WitnessArgs::new_builder()
            .lock(Some(gw_types::bytes::Bytes::from(signature)).pack())
            .build()
            .as_slice())
    };
    let mut sig = bytes(&signature(&hx(message), 1)).unwrap();
    sig[64] -= 27;
    tx.witnesses = vec![witness(sig), witness(omni_signature(&[0; 65]))];
    sign_group(&mut tx, &[1], 1, true);
    println!(
        "Deployed v0 ETH-account lock plus standard OmniLock: {} cycles",
        node.vm(&tx).unwrap()
    );
    let digest: [u8; 32] =
        Keccak256::digest([b"\x19TRON Signed Message:\n32".as_slice(), &message].concat()).into();
    let (id, sig) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(digest), &key(1))
        .serialize_compact();
    tx.witnesses[0] = witness([sig.as_slice(), &[i32::from(id) as u8]].concat());
    let rejection = node.vm(&tx).unwrap_err().to_string();
    assert!(
        rejection.contains("Inputs[0].Lock") && rejection.contains("error code 43"),
        "{rejection}"
    );
    println!("TRON-prefixed signature rejected by v0 Ethereum account lock: {rejection}");
}

#[test]
fn v0_tron_identity_requires_omnilock_tron_mode() {
    let (mut node, deps, inputs, assets, r) = setup_claim();
    let q = claim::construct(&r, inputs.clone(), &assets, &deps, 1000).unwrap();
    let mut tron_lock = claim::owner(&r.address).unwrap();
    tron_lock.args.replace_range(2..4, "03");
    let funding = base_tx(
        inputs
            .iter()
            .map(|c| CellOutput {
                lock: tron_lock.clone(),
                ..c.output.clone()
            })
            .collect(),
        inputs.iter().map(|c| c.output_data.clone()).collect(),
    );
    let points: Vec<_> = (0..3).map(|i| point(&funding, i)).collect();
    node.insert(funding);
    let mut tx = q.transaction;
    tx.inputs = points
        .iter()
        .map(|p| json!({"since":"0x0","previous_output":p}))
        .collect();
    let message = claim::signing_message(&tx).unwrap();
    let digest: [u8; 32] =
        Keccak256::digest([b"\x19TRON Signed Message:\n32".as_slice(), &message].concat()).into();
    let (id, sig) = Secp256k1::new()
        .sign_ecdsa_recoverable(&Message::from_digest(digest), &key(1))
        .serialize_compact();
    let sig = [sig.as_slice(), &[i32::from(id) as u8]].concat();
    tx.witnesses[0] = hx(packed::WitnessArgs::new_builder()
        .lock(Some(gw_types::bytes::Bytes::from(omni_signature(&sig))).pack())
        .build()
        .as_slice());
    println!(
        "Standard OmniLock TRON identity: {} cycles",
        node.vm(&tx).unwrap()
    );
    sign_group(&mut tx, &[0, 1, 2], 1, true);
    let error = node.vm(&tx).unwrap_err().to_string();
    assert!(
        error.contains("Inputs[0].Lock") && error.contains("error code -31"),
        "{error}"
    );
    // Current Ethereum-only production builder must not silently accept a TRON identity cell.
    let tron_inputs: Vec<_> = points.iter().map(|p| node.live[p].clone()).collect();
    let mut request = r;
    request.inputs = points;
    assert!(claim::construct(&request, tron_inputs, &assets, &deps, 1000).is_err());
}
