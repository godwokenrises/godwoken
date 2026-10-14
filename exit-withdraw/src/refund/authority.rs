//! Reauthenticate reviewed V0 control-wallet membership from the frozen DB.
use super::state::{word, Reader};
use crate::model::{bytes, hash32, hx};
use anyhow::{ensure, Context, Result};
use num_bigint::BigUint;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};
fn address(w: [u8; 32]) -> Result<[u8; 20]> {
    ensure!(w[..12] == [0; 12], "Noncanonical owner address");
    Ok(w[12..].try_into()?)
}
fn map_key(owner: &[u8; 20], slot: u64) -> [u8; 32] {
    let mut k = [0; 64];
    k[12..32].copy_from_slice(owner);
    k[32..].copy_from_slice(&word(slot));
    Keccak256::digest(k).into()
}
fn array_key(slot: u64, index: u64) -> Result<[u8; 32]> {
    let n = BigUint::from_bytes_be(&Keccak256::digest(word(slot))) + index;
    let b = n.to_bytes_be();
    ensure!(b.len() <= 32, "Array offset overflow");
    let mut out = [0; 32];
    out[32 - b.len()..].copy_from_slice(&b);
    Ok(out)
}
pub fn v0(reader: &Reader, accounts: &[Value]) -> Result<Value> {
    ensure!(
        matches!(reader, Reader::V0(_)),
        "V0 authorities require V0 DB"
    );
    let by_address = accounts
        .iter()
        .map(|a| Ok((a["l2_short_address"].as_str().context("short address")?, a)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut rows = vec![];
    for (id, runtime, kind) in [
        (
            16045,
            "0xa2c29fb8b77ec28f3c5b51ccc032fce33011201a1189450b06ac5c344436126c",
            "signer-wallet",
        ),
        (
            36620,
            "0xe1d48243c4954f4379c7ca0a5924adb173080d96fff87e9f3a795c573c7cabfa",
            "classic",
        ),
        (
            36630,
            "0xe1d48243c4954f4379c7ca0a5924adb173080d96fff87e9f3a795c573c7cabfa",
            "classic",
        ),
        (
            38316,
            "0x9724af9bb6361190cbe87ffcc359d99ad1ace2949050252b6995b53901be47d5",
            "safe",
        ),
        (
            48851,
            "0xa2c29fb8b77ec28f3c5b51ccc032fce33011201a1189450b06ac5c344436126c",
            "signer-wallet",
        ),
    ] {
        let account = accounts
            .get(id as usize)
            .context("Control account missing")?;
        ensure!(
            account["account_id"] == id && account["runtime_hash"] == runtime,
            "Control runtime changed {id}"
        );
        let mut owners = vec![];
        let mut extra = json!({});
        if kind == "safe" {
            ensure!(
                reader.number(id, 3)? == 3u8.into() && reader.number(id, 4)? == 2u8.into(),
                "Safe threshold/count changed"
            );
            let mut sentinel = [0; 20];
            sentinel[19] = 1;
            let mut current = sentinel;
            for _ in 0..3 {
                current = address(reader.value(id, map_key(&current, 2))?)?;
                ensure!(
                    current != sentinel && current != [0; 20],
                    "Early Safe owner terminator"
                );
                owners.push(current);
            }
            ensure!(
                address(reader.value(id, map_key(&current, 2))?)? == sentinel,
                "Extra Safe owners"
            );
            ensure!(
                address(reader.value(id, map_key(&sentinel, 1))?)? == sentinel,
                "Active Safe module"
            );
            let guard = reader.value(
                id,
                hash32("0x4a204f620c8c5ccdca3fd54d003badd85ba500436a431f0cbda4f558c93c34c8")?,
            )?;
            ensure!(guard == [0; 32], "Active Safe guard");
            let implementation = address(reader.value(id, word(0))?)?;
            let implementation_address = hx(implementation);
            let implementation = by_address
                .get(implementation_address.as_str())
                .context("Unknown Safe implementation")?;
            ensure!(
                implementation["kind"] == "polyjuice"
                    && implementation["account_id"] == 38186
                    && implementation["runtime_hash"]
                        == "0xb4e61b1b28601398753f601fec8780534bb4f1ea375143d9fb485db6dbeb57a6",
                "Missing Safe implementation code"
            );
            extra = json!({"implementation_account_id":implementation["account_id"],"implementation_runtime_hash":implementation["runtime_hash"],"active_modules":false,"guard":hx(guard)});
        } else {
            let (array_slot, member_slot) = if kind == "classic" { (3, 2) } else { (1, 0) };
            ensure!(
                reader.number(id, array_slot)? == 3u8.into(),
                "Owner count changed"
            );
            if kind == "classic" {
                ensure!(
                    reader.number(id, 4)? == 2u8.into(),
                    "Classic threshold changed"
                );
            } else {
                ensure!(
                    reader.number(id, 2)? == 1u8.into()
                        && reader.value(id, word(3))? == hash32(crate::v0::ETH)?,
                    "Signer-wallet initialization or validator changed"
                );
            }
            for i in 0..3 {
                let owner = address(reader.value(id, array_key(array_slot, i)?)?)?;
                ensure!(
                    reader.value(id, map_key(&owner, member_slot))? == word(1),
                    "Array owner is not an authorized member"
                );
                owners.push(owner);
            }
        }
        ensure!(
            owners.iter().collect::<BTreeSet<_>>().len() == 3,
            "Duplicate owners"
        );
        let mut identities = vec![];
        for owner in owners {
            let short = hx(owner);
            let account = by_address
                .get(short.as_str())
                .context("Unregistered controller")?;
            ensure!(account["kind"] == "eth_eoa", "Non-ETH controller deferred");
            ensure!(
                bytes(account["owner_eth_address"].as_str().context("ETH owner")?)?.len() == 20,
                "ETH owner width"
            );
            identities.push(json!({"account_id":account["account_id"],"l2_short_address":short,"eth_address":account["owner_eth_address"]}));
        }
        rows.push(json!({"account_id":id,"runtime_hash":runtime,"wallet_kind":kind,"members":3,"refund_threshold":2,"owners":identities,"additional_checks":extra,"original_rule":if kind=="signer-wallet"{"caller and cosigner may coincide; distinct 2-of-3 is the separately approved refund policy"}else{"stored threshold 2-of-3"},"payout_signatures_still_required":true}));
    }
    Ok(json!(rows))
}

/// V1 exit authorities, freshly authenticated; no owner address is inferred from a contract address.
pub fn v1(reader:&Reader,accounts:&[Value])->Result<Value>{
    let Reader::V1(db)=reader else{anyhow::bail!("V1 authorities require V1 DB")};
    let by_address=accounts.iter().filter_map(|a|a["address"].as_str().map(|s|(s,a))).collect::<BTreeMap<_,_>>();
    let identity=|owner:[u8;20],allow_unregistered_signer:bool|->Result<Value>{
        let address=hx(owner);
        if let Some(a)=by_address.get(address.as_str()) {
            ensure!(a["kind"]=="eth_eoa" && a["owner_eth_address"]==address,"V1 authority is not an authenticated ETH EOA");
            Ok(json!({"account_id":a["account_id"],"eth_address":address,"identity_kind":"registered_eth_eoa"}))
        } else {
            // These signer-wallet runtimes use ecrecover against their stored
            // Ethereum signer set; prior L2 registration is not required.
            // Single-owner business exits retain the stricter EOA census check.
            ensure!(allow_unregistered_signer && owner!=[0;20],"Unregistered authority is not an approved signer-wallet key: {address}");
            Ok(json!({"account_id":null,"eth_address":address,"identity_kind":"stored_eth_signer_without_l2_account","signature_still_required":true}))
        }
    };
    let mut rows=vec![];
    for (id,hash) in [(225,"0x9d1fdfcc1f4a687253d32e25e738c94850718e2690fda97b69b6989e62ead263"),(253,"0xc6bce90f1fba741db86e38bf91f918a8f39b179fd8315484c4333d33821a8c68"),(263,"0x3142438bff01104e8448c4c43ce985eecd4b24cb3b35d9140a216317807d085a")]{
        ensure!(accounts[id as usize]["runtime_hash"]==hash,"Signer wallet code changed");
        ensure!(reader.number(id,1)?==3u8.into() && reader.number(id,2)?==1u8.into(),"Signer wallet count or initialization changed");
        if id!=225{let mut domain=[0;32];domain[..13].copy_from_slice(b"GW-V1-MAINNET");domain[31]=26;ensure!(reader.value(id,word(14))?==domain,"Signer network domain changed");}
        let mut owners=vec![];let mut seen=BTreeSet::new();
        for index in 0..3{let owner=address(reader.value(id,array_key(1,index)?)?)?;ensure!(seen.insert(owner),"Duplicate signer");ensure!(reader.value(id,map_key(&owner,0))?==word(1),"Signer not authorized");owners.push(identity(owner,true)?);}
        rows.push(json!({"account_id":id,"wallet_kind":"signer-wallet","runtime_hash":hash,"members":3,"refund_threshold":2,"owners":owners,"original_allows_same_member":id!=263,"payout_signatures_still_required":true}));
    }
    for (id,count,threshold,implementation_hash,handler) in [(3943,4,2,"0x7534d039a623c146fa2b59ecf9ab1a456342b45fcdade7809f489c1fd82808b0",3935),(10506,6,3,"0x61ac213298bf09251d4920b8bd58bc8ae3416179be459354f8ae48258172419c",10478)]{
        ensure!(accounts[id as usize]["runtime_hash"]=="0x9724af9bb6361190cbe87ffcc359d99ad1ace2949050252b6995b53901be47d5","Safe proxy changed");
        ensure!(reader.number(id,3)?==BigUint::from(count as u32) && reader.number(id,4)?==BigUint::from(threshold as u32),"Safe threshold changed");
        let implementation=hx(address(reader.value(id,word(0))?)?);let implementation=by_address.get(implementation.as_str()).context("Safe implementation missing")?;
        ensure!(implementation["runtime_hash"]==implementation_hash,"Safe implementation changed");
        let mut sentinel=[0;20];sentinel[19]=1;let mut current=sentinel;let mut owners=vec![];let mut seen=BTreeSet::new();
        for _ in 0..count{current=address(reader.value(id,map_key(&current,2))?)?;ensure!(current!=sentinel && current!=[0;20] && seen.insert(current),"Invalid Safe owner list");owners.push(identity(current,false)?);}
        ensure!(address(reader.value(id,map_key(&current,2))?)?==sentinel,"Extra Safe owner");
        ensure!(address(reader.value(id,map_key(&sentinel,1))?)?==sentinel,"Active Safe module");
        ensure!(reader.value(id,hash32("0x4a204f620c8c5ccdca3fd54d003badd85ba500436a431f0cbda4f558c93c34c8")?)?==[0;32],"Active Safe guard");
        let fallback=hx(address(reader.value(id,hash32("0x6c9a6c4a39284e37ed1cf53d337577d14212a4870fb976a4366c693b939918d5")?)?)?);
        ensure!(accounts[handler]["address"]==fallback,"Safe fallback changed");
        rows.push(json!({"account_id":id,"wallet_kind":"safe","members":count,"refund_threshold":threshold,"owners":owners,"implementation_account_id":implementation["account_id"],"implementation_runtime_hash":implementation_hash,"fallback_account_id":handler,"active_modules":false,"active_guard":false,"payout_signatures_still_required":true}));
    }
    let mut getter=super::evm::FrozenDb::new(db,accounts)?;
    for (id,owner_id) in [(11784,11402),(4573,122),(5793,5662),(4075,3559)]{
        let owner=if id==4075{address(reader.value(id,word(1))?)?}else{
            let target=crate::model::eth_address(accounts[id as usize]["address"].as_str().context("Controlled account")?)?;
            let out=super::evm::call(&mut getter,target,&Keccak256::digest(b"owner()")[..4])?;address(out.as_slice().try_into().context("Owner getter width")?)?
        };
        let owner=identity(owner,false)?;ensure!(owner["account_id"]==owner_id,"Approved owner changed {id}");
        rows.push(json!({"account_id":id,"wallet_kind":if id==4075{"fixed-recipient-executor"}else{"single-owner"},"members":1,"refund_threshold":1,"owners":[owner],"executor_may_redirect":false,"payout_signatures_still_required":true}));
    }
    Ok(json!(rows))
}
