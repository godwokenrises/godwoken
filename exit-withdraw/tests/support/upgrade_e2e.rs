use super::*;
use godwoken_withdraw::{
    settlement::{adapter::Profile, signing},
    upgrade,
};
pub(super) fn exercise(mut node: Node, mut deployment: Deployment) {
    let profile = Profile::load(&deployment).unwrap();
    let code_type = hx(profile.base.rollup.code_hash().as_slice());
    let old = node
        .live
        .values()
        .find(|c| {
            c.output.type_script.as_ref().and_then(|s| s.hash().ok()) == Some(code_type.clone())
        })
        .unwrap()
        .clone();
    let mut cfg = vec![0, 0, 2, 3];
    for k in 7..=9 {
        cfg.extend_from_slice(
            &hash(&PublicKey::from_secret_key(&Secp256k1::new(), &key(k)).serialize())[..20],
        );
    }
    let lock = script(contract::MULTISIG, hash(&cfg)[..20].to_vec());
    let mut output = old.output.clone();
    output.lock = lock.clone();
    let funding = CellOutput {
        capacity: format!("0x{:x}", 200 * CAP),
        lock,
        type_script: None,
    };
    let tx = base_tx(
        vec![output, funding],
        vec![old.output_data.clone(), "0x".into()],
    );
    let code = point(&tx, 0);
    let fund = point(&tx, 1);
    node.insert(tx);
    // Only TYPE_ID + multisig execute during code replacement. New code is data.
    let mut binary = bytes(&old.output_data).unwrap();
    if !binary.starts_with(b"\x7fELF") {
        // VM bytecode is supplied by the protocol-specific settlement regression script.
        return;
    }
    // ELF permits trailing data: change the code-cell hash without changing its executable program.
    binary.extend_from_slice(b"V3-upgrade-fixture");
    deployment.validator_data_hash = hx(hash(&binary));
    let mut dependencies = deployment.cell_deps.clone();
    if !dependencies.iter().any(|d| {
        node.live[&d.out_point]
            .output
            .type_script
            .as_ref()
            .and_then(|t| t.hash().ok())
            == Some(hx(contract::MULTISIG))
    }) {
        let v: Value =
            serde_json::from_str(include_str!("../fixtures/mainnet-multisig-type.json")).unwrap();
        let ty: Script = serde_json::from_value(v["type"].clone()).unwrap();
        let data = ckb_system_scripts::BUNDLED_CELL
            .get("specs/cells/secp256k1_blake160_multisig_all")
            .unwrap();
        let t = base_tx(
            vec![CellOutput {
                capacity: format!("0x{:x}", 1000000 * CAP),
                lock: script([0; 32], vec![]),
                type_script: Some(ty),
            }],
            vec![hx(data.as_ref())],
        );
        dependencies.push(Dependency {
            out_point: point(&t, 0),
            dep_type: "code".into(),
            data_hash: hx(hash(data.as_ref())),
        });
        node.insert(t);
    }
    let r = upgrade::Request {
        protocol: profile.protocol(),
        code_cell: code,
        old_data_hash: hx(hash(&bytes(&old.output_data).unwrap())),
        multisig_config: hx(cfg),
        funding_inputs: vec![fund],
        dependencies,
        fee_shannons: 1000000,
    };
    let sk = upgrade::build(&r, &deployment, &binary, &node).unwrap();
    let req = upgrade::signing_request(&sk).unwrap();
    let group = &req.groups[0];
    let temp = tempfile::tempdir().unwrap();
    let secret = temp.path().join("synthetic-key");
    std::fs::write(&secret, hex::encode(key(7).secret_bytes())).unwrap();
    let result = std::process::Command::new("ckb-cli")
        .env("CKB_CLI_HOME", temp.path().join("cli"))
        .args([
            "--local-only",
            "--output-format",
            "json",
            "util",
            "sign-message",
            "--recoverable",
            "--message",
            &group.signing_digest,
            "--privkey-path",
        ])
        .arg(&secret)
        .output()
        .expect("upgrade integration tests require ckb-cli 1.16.0 on PATH");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: Value = serde_json::from_slice(&result.stdout).unwrap();
    let a = signing::Signatures {
        transaction_hash: req.transaction_hash.clone(),
        signatures: vec![signing::Signature {
            lock_hash: group.lock_hash.clone(),
            signature: v["signature"].as_str().unwrap().into(),
        }],
    };
    let b = signing::sign(&req, &group.lock_hash, &key(8).secret_bytes()).unwrap();
    assert!(upgrade::assemble(&sk, &[a.clone()]).is_err());
    assert!(upgrade::assemble(&sk, &[a.clone(), a.clone()]).is_err());
    let signed = upgrade::assemble(&sk, &[a, b]).unwrap();
    let cycles = node.vm(&signed).unwrap();
    assert!(cycles < 70000000);
    println!(
        "{} TYPE_ID upgrade, real ckb-cli 2-of-3 signing: {cycles} cycles",
        profile.protocol().name()
    );
    upgrade::verify(&sk, &deployment, &signed, &node).unwrap();
    let mut changed = signed.clone();
    changed.outputs[0].lock = script(contract::MULTISIG, vec![0; 20]);
    assert!(node.vm(&changed).is_err());
    assert!(upgrade::verify(&sk, &deployment, &changed, &node).is_err());
    let mut bad = r.clone();
    bad.old_data_hash = hx([0; 32]);
    assert!(upgrade::build(&bad, &deployment, &binary, &node).is_err());
    bad = r.clone();
    bad.funding_inputs.push(bad.funding_inputs[0].clone());
    assert!(upgrade::build(&bad, &deployment, &binary, &node).is_err());
    let mut tamper = sk.clone();
    tamper.transaction.outputs[1].lock = script(contract::MULTISIG, vec![0; 20]);
    assert!(upgrade::signing_request(&tamper).is_err());
}
