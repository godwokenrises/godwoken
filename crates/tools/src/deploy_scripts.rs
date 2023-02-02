use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::{Context, Result};
use ckb_fixed_hash::H256;
use clap::Parser;
use gw_types::{packed, prelude::CalcHash};
use gw_utils::local_cells::LocalCellsManager;

use crate::{
    types::{BuildScriptsResult, DeployItem, ScriptsDeploymentResult},
    utils::deploy::{DeployContext, DeployContextArgs},
};

pub const DEPLOY_SCRIPTS_COMMAND: &str = "deploy-scripts";

/// Deploy scripts used by godwoken
#[derive(Parser)]
#[clap(name = DEPLOY_SCRIPTS_COMMAND)]
pub struct DeployScriptsCommand {
    #[clap(flatten)]
    context_args: DeployContextArgs,
    /// The input json file path
    #[clap(short)]
    input_path: PathBuf,
    /// The output json file path
    #[clap(short)]
    output_path: PathBuf,
}

impl DeployScriptsCommand {
    pub async fn run(self) -> Result<()> {
        let context = self.context_args.build().await?;
        let ctx = || format!("reading input from {}", self.input_path.to_string_lossy());
        let scripts: BuildScriptsResult =
            serde_json::from_slice(&fs::read(&self.input_path).with_context(ctx)?)
                .with_context(ctx)?;
        let result = deploy_scripts(&context, &scripts).await?;
        fs::write(&self.output_path, serde_json::to_string_pretty(&result)?)
            .with_context(|| format!("write output to {}", self.output_path.to_string_lossy()))?;
        Ok(())
    }
}

pub async fn deploy_scripts(
    context: &DeployContext,
    scripts: &BuildScriptsResult,
) -> Result<ScriptsDeploymentResult> {
    // To iterator the programs by name.
    let programs: HashMap<String, PathBuf> =
        serde_json::from_value(serde_json::to_value(&scripts.programs)?)?;
    let lock: packed::Script = scripts.lock.clone().into();

    let mut local_cells = LocalCellsManager::default();
    let mut txs = Vec::new();
    let mut result = serde_json::Map::new();
    for (name, path) in programs {
        let data =
            fs::read(&path).with_context(|| format!("read file {}", path.to_string_lossy()))?;
        log::info!(
            "deploy {name}({}), size {}",
            path.to_string_lossy(),
            data.len(),
        );

        let (tx, out_point, type_script) = context
            .deploy_type_id_cell(lock.clone(), data.into(), &local_cells)
            .await?;
        let tx_hash = tx.hash();
        log::info!("tx: {:#}", H256::from(tx_hash));
        local_cells.apply_tx(&tx.as_reader());
        txs.push(tx_hash);
        result.insert(
            name,
            serde_json::to_value(&DeployItem {
                script_type_hash: type_script.hash().into(),
                type_script: type_script.into(),
                cell_dep: ckb_jsonrpc_types::CellDep {
                    out_point: out_point.into(),
                    dep_type: ckb_jsonrpc_types::DepType::Code,
                },
            })?,
        );
    }

    for tx in txs {
        log::info!("waiting for tx {:#}", H256::from(tx));
        context
            .ckb_client
            .wait_tx_committed_with_timeout_and_logging(tx, 180)
            .await?;
    }

    Ok(serde_json::from_value(result.into())?)
}
