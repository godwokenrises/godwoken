//! Real-DB-facts regression: references are opened only after the waterfall runs.
use anyhow::{Context,Result};
use godwoken_withdraw::refund::v1_closeout;
use num_bigint::BigUint;
use serde_json::{json,Value};
use std::{fs,path::PathBuf,collections::BTreeMap};
fn load(p:impl AsRef<std::path::Path>)->Result<Value>{Ok(serde_json::from_slice(&fs::read(p)?)?)}
fn quote(v:&Value)->Result<String>{let s=v.as_str().context("Quote")?;let (w,f)=s.split_once('.').unwrap_or((s,""));anyhow::ensure!(f.len()<=36,"Quote precision");Ok(format!("{w}{f}{}","0".repeat(36-f.len())).parse::<BigUint>()?.to_string())}
#[test]
#[ignore="requires freshly computed DB lending facts, account census and comparison-only references"]
fn v1_pool_local_waterfall_matches_reference()->Result<()>{
 let facts=load(std::env::var("GW_LENDING_FACTS")?)?;let state=load(std::env::var("GW_V1_FACTS")?)?;
 let result=v1_closeout::calculate(&facts,state["protocol_state"]["accounts"].as_array().context("Accounts")?)?;
 // Calculate first; never use the baseline as an allocation input.
 let dir=PathBuf::from(std::env::var("GW_CLOSEOUT_REFERENCE")?);
 let pools=load(dir.join("pools.json"))?;let sources=load(dir.join("source-checks.json"))?;let lots=load(dir.join("allocation-lots.json"))?;let mut differences=vec![];
 for r in pools.as_array().context("Pools")?{
  let a=result["pools"].as_array().unwrap().iter().find(|a|a["pool_account_id"]==r["pool"]).context("Pool missing")?;
  for (old,new) in [("ordinary_equity_quote","ordinary_equity_value_raw"),("protocol_treasury_claim_quote","treasury_equity_value_raw"),("explicit_reserve_absorbs_quote","reserve_absorption_value_raw"),("treasury_absorbs_quote","treasury_absorption_value_raw"),("treasury_remaining_quote","treasury_remaining_value_raw"),("ordinary_shortfall_pre_rounding_quote","ordinary_shortfall_value_raw")]{if a[new]!=quote(&r[old])?{differences.push(json!({"pool":r["pool"],"field":new,"generated":a[new],"reference":quote(&r[old])?}));}}
 }
 let mut actual=BTreeMap::new();
 for m in result["markets"].as_array().unwrap(){if m["pool_account_id"]==4757{continue;}
  for c in m["claims"].as_array().unwrap(){let key=(m["pool_account_id"].as_u64().unwrap(),m["market_account_id"].as_u64().unwrap(),c["address"].as_str().unwrap().to_owned());assert!(actual.insert(key,c["raw"].as_str().unwrap().to_owned()).is_none());}
 }
 let mut expected=BTreeMap::new();for r in lots.as_array().context("Lots")?{let key=(r["pool"].as_u64().unwrap(),r["source_account"].as_u64().unwrap(),r["address"].as_str().unwrap().to_owned());assert!(expected.insert(key,r["quantity_raw"].as_str().unwrap().to_owned()).is_none());}
 for k in actual.keys().chain(expected.keys()).collect::<std::collections::BTreeSet<_>>(){if actual.get(k)!=expected.get(k){differences.push(json!({"lot":k,"generated":actual.get(k),"reference":expected.get(k)}));}}
 for r in sources.as_array().context("Sources")?{
  let a=result["markets"].as_array().unwrap().iter().find(|a|a["market_account_id"]==r["source_account"]).context("Source missing")?;
  for (old,new) in [("original_cash_raw","cash_raw"),("assigned_raw","allocated_trial_raw"),("retained_after_all_attributed_claims_raw","retained_trial_raw"),("unexplained_positive_component_reserved_raw","protected_component_raw")]{if a[new]!=r[old]{differences.push(json!({"source":r["source_account"],"field":new,"generated":a[new],"reference":r[old]}));}}
 }
 let out=PathBuf::from(std::env::var("GW_CLOSEOUT_OUTPUT")?);anyhow::ensure!(!out.exists(),"Output exists");fs::write(out,serde_json::to_vec_pretty(&json!({"all_compared_equal":differences.is_empty(),"differences":differences,"pools_compared":pools.as_array().unwrap().len(),"allocation_lots_compared":expected.len(),"reconstruction":result}))?)?;
 assert!(differences.is_empty(),"Waterfall differs: {}",differences.len());Ok(())
}
