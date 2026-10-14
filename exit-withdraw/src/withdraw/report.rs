//! Current recovery balances, authenticated against the reconstructed live root.
use super::*;
use crate::db::Database;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn state_report(
    db: &Database,
    rpc: &impl chain::Chain,
    d: &Deployment,
    bundle: &Bundle,
    addresses: &[String],
) -> Result<Value> {
    let profile = d.profile()?;
    let (_, cfg) = bundle.validate()?;
    let (state, journal) = chain::reconstruct(
        db,
        rpc,
        &profile,
        &cfg.packed,
        &bundle.rollup_cell,
        d.confirmations,
    )?;
    let mut seen = BTreeSet::new();
    let mut users = Vec::new();
    for raw in addresses {
        let address = eth_address(raw)?;
        ensure!(seen.insert(address), "Duplicate report address");
        let identity = db.lookup_eth(&address)?;
        let mut status = "unregistered";
        if identity["status"] == "registered" {
            let script: Script = serde_json::from_value(identity["script"].clone())?;
            let expected = [hash32(ROLLUP_HASH)?.as_slice(), address.as_slice()].concat();
            status = if script.hash_type == "type"
                && bytes(&script.args)? == expected
                && profile.eoa_codes.contains(&hash32(&script.code_hash)?)
            {
                "eligible"
            } else {
                "unsupported_account"
            };
        }
        // Read balances even for unregistered addresses; do not equate absence of
        // an eligible identity with absence of a state balance.
        let mut balances = serde_json::Map::new();
        for (id, _) in &profile.catalogue {
            balances.insert(
                id.to_string(),
                json!(
                    contract::amount(state.read(contract::balance_key(*id, address))?).to_string()
                ),
            );
        }
        users.push(json!({"eth_address":hx(address),"status":status,"identity":identity,"balances":balances}));
    }
    let assets=profile.catalogue.iter().map(|(id,h)|Ok(json!({"asset_id":id,"type_hash":hx(h),"supply":contract::amount(state.read(contract::supply_key(*id))?).to_string()}))).collect::<Result<Vec<_>>>()?;
    ensure!(
        chain::same_cell(
            &chain::live(rpc, &bundle.rollup_cell.out_point)?,
            &bundle.rollup_cell
        ),
        "Rollup changed during state report"
    );
    Ok(
        json!({"schema_version":1,"root":hx(state.root()),"account_count":db.tip.account_count,"rollup_outpoint":bundle.rollup_cell.out_point,"profile_hash":d.profile_hash,"journal":journal,"users":users,"assets":assets}),
    )
}
