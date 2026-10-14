use exit_withdraw_state_validator::{settlement::records::*, wire};
use gw_types::prelude::Entity;
fn golden() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/settlement-v2-vectors.json")).unwrap()
}
fn raw(v: &serde_json::Value, n: &str) -> Vec<u8> {
    hex::decode(v["records"][n]["bytes"].as_str().unwrap()).unwrap()
}
#[test]
fn fixed_molecule_records_match_independent_golden_vectors() {
    let v = golden();
    macro_rules! check {
        ($name:literal,$ty:ty,$wire:ty) => {{
            let b = raw(&v, $name);
            let parsed = decode::<$ty>(&b).unwrap();
            assert_eq!(encode(&parsed), b);
            assert_eq!(<$ty>::SIZE, b.len());
            assert_eq!(
                hex::encode(commitment(&b)),
                v["records"][$name]["commitment"]
            );
            assert_eq!(<$wire>::from_slice(&b).unwrap().as_slice(), b);
            let mut extended = b.clone();
            extended.push(0);
            assert!(decode::<$ty>(&extended).is_err());
            assert!(decode::<$ty>(&b[..b.len() - 1]).is_err());
            let mut tag = b;
            tag[0] ^= 1;
            assert!(decode::<$ty>(&tag).is_err());
        }};
    }
    check!("source", Source, wire::SettlementSource);
    check!("lot", Lot, wire::SettlementLot);
    check!("reserve", Reserve, wire::SettlementReserve);
    check!("claim", Claim, wire::SettlementClaim);
    check!("source_budget", SourceBudget, wire::SettlementSourceBudget);
    check!("asset_limit", AssetLimit, wire::SettlementAssetLimit);
    check!("asset_budget", AssetBudget, wire::SettlementAssetBudget);
    check!("epoch", Epoch, wire::SettlementEpoch);
    check!("paid", Paid, wire::SettlementPaid);
    check!("permit", Permit, wire::SettlementPermit);
    check!("payment", Payment, wire::SettlementPayment);
    check!("metadata", Metadata, wire::SettlementMetadata);
    assert_eq!(commitment(&[]), [0; 32]);
}
#[test]
fn keys_are_domain_deployment_and_epoch_separated() {
    let v = golden();
    let d = [0x11; 32];
    let src = decode::<Source>(&raw(&v, "source")).unwrap();
    let lot = decode::<Lot>(&raw(&v, "lot")).unwrap();
    let claim = decode::<Claim>(&raw(&v, "claim")).unwrap();
    for (n, k) in [
        ("source_key", source_key(d, &src)),
        ("lot_key", lot_key(d, &lot)),
        ("claim_key", claim_key(d, &claim)),
        ("epoch_key", epoch_key(d, 9)),
        ("asset_key", asset_key(d, 1)),
        ("metadata_key", metadata_key(d, [0x42; 32])),
    ] {
        assert_eq!(hex::encode(k), v[n]);
    }
    assert_ne!(source_key(d, &src), source_key([0x12; 32], &src));
    let mut later = claim;
    later.epoch += 1;
    assert_ne!(claim_key(d, &claim), claim_key(d, &later));
    assert_ne!(epoch_key(d, 1), asset_key(d, 1));
}
