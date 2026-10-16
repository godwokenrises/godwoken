//! Explicit protocol boundaries for offline state, custody and settlement envelopes.
use crate::{
    db::Database,
    model::*,
    v0::V0Db,
    withdraw::{state::RecoveryState, Deployment},
};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{self as c, native::NativeVersion, settlement as s};
use gw_common::state::{build_script_hash_to_account_id_key, GW_ACCOUNT_SCRIPT_HASH_TYPE};
use gw_types::{packed, prelude::*};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Deref, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    V1,
    V0,
}
impl Protocol {
    pub fn is_v1(&self) -> bool {
        *self == Self::V1
    }
    pub fn native(self) -> NativeVersion {
        match self {
            Self::V1 => NativeVersion::V1,
            Self::V0 => NativeVersion::V0,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V0 => "v0",
        }
    }
    pub fn rollup(self) -> &'static str {
        match self {
            Self::V1 => ROLLUP_HASH,
            Self::V0 => crate::v0::ROLLUP,
        }
    }
}
pub trait NativeDb {
    fn protocol(&self) -> Protocol;
    fn root(&self) -> String;
    fn account(&self, id: u32) -> Result<(c::Hash, Script)>;
    fn script(&self, hash: &c::Hash, l1: bool) -> Result<Option<Script>>;
    fn lookup_eth(&self, address: &[u8; 20]) -> Result<serde_json::Value>;
    fn overlay(&self) -> Result<RecoveryState<'_>>;
    fn anchor(&self, state: &packed::GlobalState) -> Result<()>;
}
impl NativeDb for Database {
    fn protocol(&self) -> Protocol {
        Protocol::V1
    }
    fn root(&self) -> String {
        Database::root(self)
    }
    fn account(&self, id: u32) -> Result<(c::Hash, Script)> {
        Database::account(self, id)
    }
    fn script(&self, h: &c::Hash, l1: bool) -> Result<Option<Script>> {
        Database::script(self, h, l1)
    }
    fn lookup_eth(&self, a: &[u8; 20]) -> Result<serde_json::Value> {
        Database::lookup_eth(self, a)
    }
    fn overlay(&self) -> Result<RecoveryState<'_>> {
        RecoveryState::new(self)
    }
    fn anchor(&self, s: &packed::GlobalState) -> Result<()> {
        let count: u32 = s.account().count().unpack();
        ensure!(
            count == self.tip.account_count
                && s.tip_block_hash().as_slice() == hash32(&self.tip.hash)?,
            "Bootstrap metadata differs from v1 DB"
        );
        Ok(())
    }
}
impl NativeDb for V0Db {
    fn protocol(&self) -> Protocol {
        Protocol::V0
    }
    fn root(&self) -> String {
        V0Db::root(self)
    }
    fn account(&self, id: u32) -> Result<(c::Hash, Script)> {
        ensure!(
            (id as u64)
                < self.tip["account_count"]
                    .as_u64()
                    .context("Missing account count")?,
            "Account outside snapshot"
        );
        let h = self.raw(crate::v0::field(id, GW_ACCOUNT_SCRIPT_HASH_TYPE))?;
        let b = self.preimage(14, &h)?.context("Missing v0 Script")?;
        let sc = Script::from_packed(packed::Script::from_slice(&b)?)?;
        let short = h[..20].try_into()?;
        c::native::NativeVersion::V0
            .identity(
                id,
                h,
                short,
                self.tip["account_count"].as_u64().unwrap() as u32,
                |k| self.raw(k).map_err(|_| c::Error::State),
            )
            .map_err(|e| anyhow::anyhow!("v0 identity for account {id}: {e:?}"))?;
        Ok((h, sc))
    }
    fn script(&self, h: &c::Hash, l1: bool) -> Result<Option<Script>> {
        self.preimage(if l1 { 15 } else { 14 }, h)?
            .map(|b| Script::from_packed(packed::Script::from_slice(&b)?))
            .transpose()
    }
    fn lookup_eth(&self, a: &[u8; 20]) -> Result<serde_json::Value> {
        let sc = packed::Script::new_builder()
            .code_hash(hash32(crate::v0::ETH)?.pack())
            .hash_type(1u8.into())
            .args([hash32(crate::v0::ROLLUP)?.as_slice(), a].concat().pack())
            .build();
        let h = c::script_hash(&sc);
        let v = self.raw(build_script_hash_to_account_id_key(&h))?;
        if v == [0; 32] {
            return Ok(serde_json::json!({"status":"unregistered"}));
        }
        ensure!(v[4..] == [0; 28], "Invalid v0 account mapping");
        let id = u32::from_le_bytes(v[..4].try_into()?);
        let (actual, script) = self.account(id)?;
        ensure!(actual == h, "ETH account mapping mismatch");
        Ok(serde_json::json!({"status":"registered","account_id":id,"script":script}))
    }
    fn overlay(&self) -> Result<RecoveryState<'_>> {
        ensure!(
            self.root() == self.tip["account_root"],
            "v0 DB root differs from tip"
        );
        Ok(RecoveryState::from_store(
            *self.tree.root(),
            self.state_store(),
        ))
    }
    fn anchor(&self, s: &packed::GlobalState) -> Result<()> {
        let count: u32 = s.account().count().unpack();
        ensure!(
            Some(count as u64) == self.tip["account_count"].as_u64()
                && hx(s.account().merkle_root().as_slice()) == self.root(),
            "v0 bootstrap account state differs from DB"
        );
        // The release authenticates an empty successor separately. Refunds depend only on
        // this exact root/count; the full bootstrap GlobalState is pinned in the profile.
        Ok(())
    }
}
pub fn open(protocol: Protocol, path: &Path) -> Result<Box<dyn NativeDb>> {
    Ok(match protocol {
        Protocol::V1 => Box::new(Database::open(path)?),
        Protocol::V0 => {
            let mut db = V0Db::open(path)?;
            db.rebuild()?;
            Box::new(db)
        }
    })
}
#[derive(Clone)]
pub enum Profile {
    V1(s::SettlementProfile),
    V0(c::v0::Profile),
}
impl Deref for Profile {
    type Target = s::SettlementProfile;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::V1(p) => p,
            Self::V0(p) => &p.settlement,
        }
    }
}
impl Profile {
    pub fn load(d: &Deployment) -> Result<Self> {
        Self::decode(&bytes(&d.profile)?, hash32(&d.profile_hash)?)
    }
    pub fn decode(b: &[u8], h: c::Hash) -> Result<Self> {
        let p = if b.starts_with(b"GWV0SET3") {
            Self::V0(c::v0::decode_profile(b, h).map_err(|e| anyhow::anyhow!("v0 profile: {e:?}"))?)
        } else {
            Self::V1(
                s::codec::decode_profile(b, h).map_err(|e| anyhow::anyhow!("v1 profile: {e:?}"))?,
            )
        };
        ensure!(
            hx(c::script_hash(&p.base.rollup)) == p.protocol().rollup(),
            "Profile is not the selected mainnet rollup"
        );
        Ok(p)
    }
    pub fn protocol(&self) -> Protocol {
        match self {
            Self::V1(_) => Protocol::V1,
            Self::V0(_) => Protocol::V0,
        }
    }
    pub fn hash(&self) -> c::Hash {
        hash(&match self {
            Self::V1(p) => s::codec::encode_profile(p),
            Self::V0(p) => c::v0::encode_profile(p),
        })
    }
    pub fn evaluate(&self, a: &s::SettlementAction) -> Result<s::Evaluation> {
        s::evaluate_version(self, self.hash(), a, self.protocol().native())
            .map_err(|e| anyhow::anyhow!("Settlement evaluation: {e:?}"))
    }
    pub fn verify(
        &self,
        raw_cfg: &[u8],
        tx: &c::Transaction,
        a: &s::SettlementAction,
    ) -> Result<c::Outcome> {
        match self {
            Self::V1(p) => s::verify(
                p,
                self.hash(),
                &packed::RollupConfig::from_slice(raw_cfg)?,
                tx,
                a,
            ),
            Self::V0(p) => c::v0::verify(p, self.hash(), raw_cfg, tx, a),
        }
        .map_err(|e| anyhow::anyhow!("Settlement transition: {e:?}"))
    }
    pub fn encode_action(&self, a: &s::SettlementAction) -> Vec<u8> {
        match self {
            Self::V1(_) => s::codec::encode_action(a),
            Self::V0(_) => c::v0::encode_action(a),
        }
    }
    pub fn bind(&self, plan: &super::plan::Plan) -> Result<()> {
        ensure!(
            plan.protocol == self.protocol(),
            "Plan/profile protocol mismatch"
        );
        plan.bind(self)
    }
}
pub fn validate(b: &Bundle, protocol: Protocol) -> Result<(GlobalState, Config)> {
    if protocol == Protocol::V1 {
        return b.validate();
    }
    ensure!(
        b.schema_version == 1 && b.network == "mainnet_v0",
        "Wrong snapshot protocol"
    );
    ensure!(
        b.rollup_cell
            .output
            .type_script
            .as_ref()
            .context("Missing rollup type")?
            .hash()?
            == crate::v0::ROLLUP,
        "Wrong v0 rollup"
    );
    let global = GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?;
    let raw = bytes(&b.config_data)?;
    ensure!(
        hx(hash(&raw)) == global.config_hash,
        "Config commitment mismatch"
    );
    let packed = c::v0::config(&raw).map_err(|e| anyhow::anyhow!("v0 config: {e:?}"))?;
    let hashes = [
        ("l1_sudt", packed.l1_sudt_script_type_hash()),
        ("l2_sudt", packed.l2_sudt_validator_script_type_hash()),
        ("custodian", packed.custodian_script_type_hash()),
        ("deposit", packed.deposit_script_type_hash()),
        ("withdrawal", packed.withdrawal_script_type_hash()),
        ("stake", packed.stake_script_type_hash()),
        ("challenge", packed.challenge_script_type_hash()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), hx(v.as_slice())))
    .collect::<BTreeMap<_, _>>();
    ensure!(
        b.cells.len() == FAMILIES.len() && FAMILIES.iter().all(|f| b.cells.contains_key(*f)),
        "Incomplete cell inventory"
    );
    let mut points = std::collections::BTreeSet::new();
    points.insert(b.rollup_cell.out_point.clone());
    for (family, cells) in &b.cells {
        for cell in cells {
            ensure!(
                points.insert(cell.out_point.clone())
                    && cell.output.lock.code_hash == hashes[family]
                    && cell.output.lock.hash_type == "type"
                    && bytes(&cell.output.lock.args)?.starts_with(&hash32(crate::v0::ROLLUP)?),
                "Wrong or duplicated v0 cell"
            );
        }
    }
    Ok((
        global,
        Config {
            finality_blocks: packed.finality_blocks().unpack(),
            packed,
            hashes,
        },
    ))
}
pub fn bundle(value: serde_json::Value, protocol: Protocol) -> Result<Bundle> {
    let mut value = value;
    if protocol == Protocol::V0 {
        let o = value.as_object_mut().context("Snapshot object")?;
        o.insert("schema_version".into(), 1.into());
        o.entry("observed_start").or_insert("".into());
        o.entry("observed_end").or_insert("".into());
    }
    let b = serde_json::from_value(value)?;
    validate(&b, protocol)?;
    Ok(b)
}
pub fn finalized(cell: &Cell, b: &Bundle, cfg: &Config, protocol: Protocol) -> Result<bool> {
    let gs = GlobalState::parse(&bytes(&b.rollup_cell.output_data)?)?;
    let args = bytes(&cell.output.lock.args)?;
    let raw = args.get(32..).context("Invalid custodian args")?;
    if protocol == Protocol::V1 {
        let ca = packed::CustodianLockArgs::from_slice(raw)?;
        Ok(crate::audit::finalized(
            ca.deposit_finalized_timepoint().unpack(),
            &gs,
            cfg,
        ))
    } else {
        let f = c::v0::table::<3>(raw).map_err(|e| anyhow::anyhow!("Custodian: {e:?}"))?;
        Ok(
            u64::from_le_bytes(f[2].try_into()?) <= gs.finalized_timepoint
                && gs.finalized_timepoint < 1u64 << 63,
        )
    }
}
