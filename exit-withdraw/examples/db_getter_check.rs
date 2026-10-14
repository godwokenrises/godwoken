//! Development verification: execute deployed V1 getters from DB, no reference inputs.
use anyhow::{ensure, Context, Result};
use godwoken_withdraw::{
    model::{eth_address, hx},
    refund::{evm, state::Reader},
    report::write_new,
    settlement::adapter::Protocol,
};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::path::Path;
fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().collect();
    ensure!((3..=4).contains(&a.len()), "DB NEW_OUTPUT [FRESH_PROTOCOL_STATE_FOR_GETTER_REGRESSION]");
    let reader = Reader::open(Protocol::V1, Path::new(&a[1]))?;
    let accounts = reader.accounts()?;
    let Reader::V1(db) = &reader else {
        unreachable!()
    };
    if let Some(path)=a.get(3) {
        let facts:serde_json::Value=serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(facts["root"]==db.root() && facts["protocol"]=="v1","Mixed DB facts");
        let rows=facts["protocol_state"]["rows"].as_array().context("Fresh state rows")?;
        let ledgers=godwoken_withdraw::refund::v1_ledgers::extract(&reader,&accounts,rows)?;
        write_new(Path::new(&a[2]),&serde_json::to_vec_pretty(&json!({"root":db.root(),"ledgers":ledgers,"diagnostic_only":true,"source":"fresh DB-state mapping facts; all positive balances and supplies independently checked with getters from DB"}))?)?;
        return Ok(());
    }
    let mut getter = evm::FrozenDb::new(db, &accounts)?;
    let mut rows = vec![];
    for (id, sig) in [
        (133, "totalSupply()"),
        (133, "sudtId()"),
        (180, "totalSupply()"),
        (187, "totalSupply()"),
        (187, "getReserves()"),
        (266, "totalShares()"),
        (10295, "exchangeRateStored()"),
        (10295, "totalBorrows()"),
        (10711, "getAuthorizer()"),
        (11784, "owner()"),
    ] {
        let target = eth_address(accounts[id]["address"].as_str().context("address")?)?;
        let result = evm::call(&mut getter, target, &Keccak256::digest(sig.as_bytes())[..4]);
        rows.push(json!({"account_id":id,"signature":sig,"result":result.as_ref().ok().map(hx),"error":result.err().map(|e|format!("{e:#}"))}));
    }
    write_new(
        Path::new(&a[2]),
        &serde_json::to_vec_pretty(
            &json!({"root":db.root(),"rows":rows,"only_db_input":true,"read_only":true}),
        )?,
    )?;
    Ok(())
}
