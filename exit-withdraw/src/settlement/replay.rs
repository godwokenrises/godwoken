//! Reconstruct exclusively from confirmed canonical rollup-spending transactions.
use super::{plan::Plan, state::Ledger};
use crate::{
    model::*,
    settlement::adapter::{NativeDb, Profile},
    withdraw::{
        chain::{Replay, *},
        state::RecoveryState,
        *,
    },
};
use anyhow::{ensure, Context, Result};
use exit_withdraw_state_validator::{self as contract, settlement as settle};
use gw_types::{packed, prelude::*};
use std::collections::BTreeSet;
pub fn action(tx: &Transaction, profile: &contract::Profile) -> Result<settle::SettlementAction> {
    let rollup = Script::from_packed(profile.rollup.clone())?;
    let i = tx
        .outputs
        .iter()
        .position(|o| o.type_script.as_ref() == Some(&rollup))
        .context("Missing output rollup")?;
    let w = bytes(
        tx.witnesses
            .get(i)
            .context("Missing rollup output witness")?,
    )?;
    let w = packed::WitnessArgsReader::from_slice(&w)?;
    let data = w
        .output_type()
        .to_opt()
        .context("Missing recovery action")?;
    settle::codec::decode_action(&unframe(tx, data.raw_data())?)
        .map_err(|e| anyhow::anyhow!("Recovery action: {e:?}"))
}
pub fn reconstruct_for<'a>(
    db: &'a dyn NativeDb,
    rpc: &impl Chain,
    sp: &Profile,
    plan: &Plan,
    cfg: &[u8],
    current: &Cell,
    confirmations: u64,
) -> Result<(RecoveryState<'a>, Ledger, Vec<Replay>)> {
    sp.bind(plan)?;
    ensure!(
        db.protocol() == sp.protocol(),
        "DB/profile protocol mismatch"
    );
    let p = &sp.base;
    let mut ledger = Ledger::new(plan)?;
    let mut state = db.overlay()?;
    ensure!(
        p.bootstrap_state.account().merkle_root().as_slice() == state.root(),
        "DB is not the profile bootstrap snapshot"
    );
    db.anchor(&p.bootstrap_state)?;
    let anchor = from_outpoint(&p.bootstrap_outpoint);
    let rollup = Script::from_packed(p.rollup.clone())?;
    let mut history = History::new(rpc, confirmations)?;
    // Also authenticate the current anchor's inclusion/confirmations, even before the first exit.
    let observed = history.cell(&current.out_point)?;
    ensure!(
        same_cell(&observed, current),
        "Current rollup differs from canonical history"
    );
    let mut cursor = current.out_point.clone();
    let mut seen = BTreeSet::new();
    let mut chain = Vec::new();
    while cursor != anchor {
        ensure!(
            chain.len() < 100_000 && seen.insert(cursor.clone()),
            "Recovery history limit or cycle"
        );
        let tx = history.transaction(&cursor.tx_hash)?;
        ensure!(
            tx.inputs.len() <= contract::MAX_INPUTS && tx.outputs.len() <= contract::MAX_OUTPUTS,
            "Historical exit exceeds contract bounds"
        );
        let inputs = tx
            .input_points()?
            .iter()
            .map(|op| history.cell(op))
            .collect::<Result<Vec<_>>>()?;
        let prev: Vec<_> = inputs
            .iter()
            .filter(|c| c.output.type_script.as_ref() == Some(&rollup))
            .collect();
        ensure!(prev.len() == 1, "History has no unique rollup predecessor");
        let point = prev[0].out_point.clone();
        let outputs: Vec<_> = tx
            .outputs
            .iter()
            .enumerate()
            .filter(|(_, o)| o.type_script.as_ref() == Some(&rollup))
            .collect();
        ensure!(
            outputs.len() == 1 && quantity(&cursor.index)? == outputs[0].0 as u64,
            "History output is not the unique rollup"
        );
        chain.push((tx, inputs, point.clone(), cursor));
        cursor = point;
    }
    let bootstrap = history.cell(&anchor)?;
    ensure!(
        bootstrap.output.type_script.as_ref() == Some(&rollup),
        "Bootstrap rollup type differs from profile"
    );
    ensure!(
        bootstrap.output.lock.packed()?.as_slice() == p.bootstrap_lock.as_slice()
            && quantity(&bootstrap.output.capacity)? == p.bootstrap_capacity
            && bytes(&bootstrap.output_data)? == p.bootstrap_state.as_slice(),
        "Bootstrap anchor bytes differ from profile"
    );
    let mut journal = Vec::new();
    for (tx, inputs, previous, next) in chain.into_iter().rev() {
        let transition = resolved(&tx, &inputs)?;
        let a = action_for(&tx, sp)?;
        let prior = inputs
            .iter()
            .find(|c| c.out_point == previous)
            .context("Missing predecessor")?;
        ensure!(
            GlobalState::parse(&bytes(&prior.output_data)?)?.account_root == hx(state.root()),
            "Recovery history is discontinuous"
        );
        // Match every witness value to locally reconstructed state before applying the verified delta.
        for (k, v) in &a.native.kv {
            ensure!(
                state.read(*k)? == *v,
                "History witness disagrees with reconstructed storage"
            );
        }
        let outcome = sp.verify(cfg, &transition, &a)?;
        let before = hx(state.root());
        let evaluated = sp.evaluate(&a)?;
        super::state::apply_native(&mut state, &evaluated, outcome.root)?;
        ledger.apply(evaluated)?;
        journal.push(Replay {
            transaction_hash: tx.hash()?,
            input_rollup: previous,
            output_rollup: next,
            before_root: before,
            after_root: hx(state.root()),
        });
    }
    ensure!(
        GlobalState::parse(&bytes(&current.output_data)?)?.account_root == hx(state.root()),
        "Reconstructed L2 root does not equal live rollup root"
    );
    history.recheck()?;
    ensure!(
        same_cell(&live(rpc, &current.out_point)?, current),
        "Rollup changed during reconstruction"
    );
    Ok((state, ledger, journal))
}

pub fn action_for(tx: &Transaction, profile: &Profile) -> Result<settle::SettlementAction> {
    if profile.protocol() == super::adapter::Protocol::V1 {
        return action(tx, &profile.base);
    }
    let rollup = Script::from_packed(profile.base.rollup.clone())?;
    let i = tx
        .outputs
        .iter()
        .position(|o| o.type_script.as_ref() == Some(&rollup))
        .context("Missing rollup")?;
    let raw = bytes(tx.witnesses.get(i).context("Missing witness")?)?;
    let w = packed::WitnessArgsReader::from_slice(&raw)?;
    let data = w.output_type().to_opt().context("Missing action")?;
    contract::v0::decode_action(&unframe(tx, data.raw_data())?)
        .map_err(|e| anyhow::anyhow!("v0 action: {e:?}"))
}

pub fn reconstruct<'a>(
    db: &'a crate::db::Database,
    rpc: &impl Chain,
    sp: &settle::SettlementProfile,
    plan: &Plan,
    cfg: &packed::RollupConfig,
    current: &Cell,
    confirmations: u64,
) -> Result<(RecoveryState<'a>, Ledger, Vec<Replay>)> {
    reconstruct_for(
        db,
        rpc,
        &Profile::V1(sp.clone()),
        plan,
        cfg.as_slice(),
        current,
        confirmations,
    )
}

pub fn unframe(tx: &Transaction, data: &[u8]) -> Result<Vec<u8>> {
    contract::transport::unframe(data, tx.inputs.len(), |i| {
        let s = tx.witnesses.get(i).ok_or(contract::Error::Encoding)?;
        bytes(s).map_err(|_| contract::Error::Encoding)
    })
    .map_err(|e| anyhow::anyhow!("Framing {e:?}"))
}
pub fn frame_transaction(tx: &mut Transaction) -> Result<()> {
    let raw = bytes(&tx.witnesses[0])?;
    let w = packed::WitnessArgs::from_slice(&raw)?;
    let data = w
        .output_type()
        .to_opt()
        .context("Missing action")?
        .raw_data();
    let (header, parts) = contract::transport::frame(&data);
    tx.witnesses[0] = hx(w
        .as_builder()
        .output_type(Some(gw_types::bytes::Bytes::from(header)).pack())
        .build()
        .as_slice());
    ensure!(
        tx.witnesses.len() == tx.inputs.len(),
        "Unexpected extra witnesses before framing"
    );
    tx.witnesses.extend(parts.iter().map(|p| hx(p)));
    Ok(())
}
