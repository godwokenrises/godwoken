use anyhow::{anyhow, bail, Result};
use ckb_chain_spec::consensus::ConsensusBuilder;
use ckb_fixed_hash::H256;
use ckb_script::{TransactionScriptsVerifier, TxVerifyEnv};
use ckb_traits::{CellDataProvider, HeaderProvider};
use ckb_types::{
    bytes::Bytes,
    core::{
        cell::{CellMeta, CellMetaBuilder, ResolvedTransaction},
        hardfork::HardForkSwitch,
        DepType, HeaderView,
    },
    packed::{Byte32, CellDep, CellInput, CellOutput, OutPoint, OutPointVec, Transaction},
    prelude::{Builder, Entity, Pack, Unpack},
};
use gw_ckb_hardfork::{GLOBAL_CURRENT_EPOCH_NUMBER, GLOBAL_HARDFORK_SWITCH};
use gw_jsonrpc_types::{
    ckb_jsonrpc_types,
    debugger::{ReprMockCellDep, ReprMockInfo, ReprMockInput, ReprMockTransaction},
};
use gw_types::offchain::InputCellInfo;

use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
};
use std::{convert::TryFrom, fs::read, path::PathBuf, sync::Arc};

pub struct TxWithContext {
    pub cell_deps: Vec<InputCellInfo>,
    pub inputs: Vec<InputCellInfo>,

    pub tx: gw_types::packed::Transaction,
}

#[derive(Clone)]
pub struct RollupCellDeps(Arc<HashMap<OutPoint, CellInfo>>);

impl RollupCellDeps {
    pub fn new(cells: Vec<gw_types::offchain::InputCellInfo>) -> Self {
        RollupCellDeps(Arc::new(cells.into_iter().map(into_info).collect()))
    }

    pub fn replace_scripts(&self, scripts: &HashMap<H256, PathBuf>) -> Result<Self> {
        let mut rollup_cell_deps = (*self.0).clone();
        let mut replaced_scripts = HashSet::with_capacity(scripts.len());

        for info in rollup_cell_deps.values_mut() {
            let type_script = match info.output.type_().to_opt() {
                Some(script) => script,
                None => continue,
            };

            let script_hash: H256 = type_script.calc_script_hash().unpack();
            let script_path = match scripts.get(&script_hash) {
                Some(path) => path,
                None => continue,
            };

            log::info!(
                "replace code hash {} with binary from {:?}",
                script_hash,
                script_path
            );

            let script = read(script_path)?;
            info.data = Bytes::from(script);
            info.data_hash = CellOutput::calc_data_hash(&info.data);

            replaced_scripts.insert(script_hash);
        }

        let scripts = scripts.keys().cloned().collect::<HashSet<H256>>();
        let missing: Vec<_> = scripts.symmetric_difference(&replaced_scripts).collect();
        if !missing.is_empty() {
            bail!("{:?} scripts not found in rollup cell deps", missing);
        }

        Ok(RollupCellDeps(Arc::new(rollup_cell_deps)))
    }
}

pub fn verify_tx(
    rollup_cell_deps: &RollupCellDeps,
    tx_with_context: TxWithContext,
    max_cycles: u64,
) -> Result<u64> {
    let mut data_loader = TxDataLoader::new(rollup_cell_deps);
    data_loader.extend_cell_deps(tx_with_context.cell_deps);
    data_loader.extend_inputs(tx_with_context.inputs);

    let resolved_tx = data_loader.resolve_tx(&tx_with_context.tx)?;

    let hardfork_switch = {
        let switch = GLOBAL_HARDFORK_SWITCH.load();
        HardForkSwitch::new_without_any_enabled()
            .as_builder()
            .rfc_0028(switch.rfc_0028())
            .rfc_0029(switch.rfc_0029())
            .rfc_0030(switch.rfc_0030())
            .rfc_0031(switch.rfc_0031())
            .rfc_0032(switch.rfc_0032())
            .rfc_0036(switch.rfc_0036())
            .rfc_0038(switch.rfc_0038())
            .build()
            .map_err(|err| anyhow!(err))?
    };
    let consensus = ConsensusBuilder::default()
        .hardfork_switch(hardfork_switch)
        .build();
    let current_epoch_number = GLOBAL_CURRENT_EPOCH_NUMBER.load(Ordering::SeqCst);
    let tx_verify_env = TxVerifyEnv::new_submit(
        &HeaderView::new_advanced_builder()
            .epoch(current_epoch_number.pack())
            .build(),
    );
    let cycles =
        TransactionScriptsVerifier::new(&resolved_tx, &consensus, &data_loader, &tx_verify_env)
            .verify(max_cycles)
            .map_err(|err| anyhow!("verify tx failed: {}", err))?;

    Ok(cycles)
}

pub fn dump_tx(
    rollup_cell_deps: &RollupCellDeps,
    tx_with_context: TxWithContext,
) -> Result<ReprMockTransaction> {
    let to_repr_input = |info: &InputCellInfo| -> ReprMockInput {
        ReprMockInput {
            input: CellInput::new_unchecked(info.input.as_bytes()).into(),
            output: CellOutput::new_unchecked(info.cell.output.as_bytes()).into(),
            data: ckb_jsonrpc_types::JsonBytes::from_bytes(info.cell.data.clone()),
            header: None,
        }
    };

    let to_repr_dep = |meta: CellMeta, dep_type: DepType| -> ReprMockCellDep {
        let cell_dep = CellDep::new_builder()
            .out_point(meta.out_point)
            .dep_type(dep_type.into())
            .build();
        let data = meta.mem_cell_data.unwrap_or_else(Bytes::new);

        ReprMockCellDep {
            cell_dep: cell_dep.into(),
            output: meta.cell_output.into(),
            data: ckb_jsonrpc_types::JsonBytes::from_bytes(data),
            header: None,
        }
    };

    let repr_inputs = tx_with_context.inputs.iter().map(to_repr_input).collect();

    let mut data_loader = TxDataLoader::new(rollup_cell_deps);
    data_loader.extend_cell_deps(tx_with_context.cell_deps);
    data_loader.extend_inputs(tx_with_context.inputs);

    let resolved_tx = data_loader.resolve_tx(&tx_with_context.tx)?;
    let repr_deps = {
        let code_deps = resolved_tx.resolved_cell_deps.into_iter();
        let to_repr = code_deps.map(|d| to_repr_dep(d, DepType::Code));
        let group_deps = resolved_tx.resolved_dep_groups.into_iter();
        to_repr.chain(group_deps.map(|d| to_repr_dep(d, DepType::DepGroup)))
    };

    let mock_info = ReprMockInfo {
        inputs: repr_inputs,
        cell_deps: repr_deps.collect(),
        header_deps: vec![],
    };

    let mock_tx = ReprMockTransaction {
        mock_info,
        tx: Transaction::new_unchecked(tx_with_context.tx.as_bytes()).into(),
    };

    Ok(mock_tx)
}

struct TxDataLoader {
    rollup_cell_deps: Arc<HashMap<OutPoint, CellInfo>>,
    headers: HashMap<Byte32, HeaderView>,
    cell_deps: HashMap<OutPoint, CellInfo>,
    inputs: HashMap<OutPoint, CellInfo>,
}

impl TxDataLoader {
    pub fn new(rollup_cell_deps: &RollupCellDeps) -> Self {
        TxDataLoader {
            rollup_cell_deps: Arc::clone(&rollup_cell_deps.0),
            headers: Default::default(),
            cell_deps: Default::default(),
            inputs: Default::default(),
        }
    }

    pub fn extend_inputs(&mut self, inputs: Vec<gw_types::offchain::InputCellInfo>) {
        self.inputs.extend(inputs.into_iter().map(into_info))
    }

    pub fn extend_cell_deps(&mut self, deps: Vec<gw_types::offchain::InputCellInfo>) {
        self.cell_deps.extend(deps.into_iter().map(into_info))
    }

    fn resolve_tx(&self, tx: &gw_types::packed::Transaction) -> Result<ResolvedTransaction> {
        let to_meta = |out_point: OutPoint| -> Result<CellMeta> {
            self.get_cell_meta(&out_point)
                .ok_or_else(|| anyhow!("resolve tx failed, unknown out point {}", out_point))
        };

        let tx = Transaction::new_unchecked(tx.as_bytes());
        let mut resolved_dep_groups = vec![];
        let mut resolved_cell_deps = vec![];

        for cell_dep in tx.raw().cell_deps().into_iter() {
            let cell_meta = to_meta(cell_dep.out_point())?;

            match DepType::try_from(cell_dep.dep_type())
                .map_err(|_| anyhow!("resolve tx invalid dep type"))?
            {
                DepType::DepGroup => {
                    let data = {
                        let to_data = cell_meta.mem_cell_data.as_ref();
                        to_data.ok_or_else(|| anyhow!("invalid dep group"))?
                    };

                    let out_points =
                        OutPointVec::from_slice(data).map_err(|_| anyhow!("invalid dep group"))?;
                    let cell_deps = out_points.into_iter().map(to_meta);

                    resolved_cell_deps.extend(cell_deps.collect::<Result<Vec<_>>>()?);
                    resolved_dep_groups.push(cell_meta)
                }
                DepType::Code => resolved_cell_deps.push(cell_meta),
            }
        }

        let resolved_inputs: Vec<CellMeta> = {
            let to_out_point = tx.raw().inputs().into_iter().map(|d| d.previous_output());
            to_out_point.map(to_meta).collect::<Result<Vec<_>>>()?
        };

        Ok(ResolvedTransaction {
            transaction: tx.into_view(),
            resolved_cell_deps,
            resolved_inputs,
            resolved_dep_groups,
        })
    }

    fn get_cell_info(&self, out_point: &OutPoint) -> Option<&CellInfo> {
        let mut info = self.rollup_cell_deps.get(out_point);
        if info.is_some() {
            return info;
        };

        info = self.cell_deps.get(out_point);
        if info.is_some() {
            return info;
        }

        self.inputs.get(out_point)
    }

    fn get_cell_meta(&self, out_point: &OutPoint) -> Option<CellMeta> {
        self.get_cell_info(out_point).map(|ci| {
            CellMetaBuilder::from_cell_output(ci.output.to_owned(), ci.data.to_owned())
                .out_point(out_point.clone())
                .build()
        })
    }
}

impl CellDataProvider for TxDataLoader {
    fn get_cell_data(&self, out_point: &OutPoint) -> Option<Bytes> {
        self.get_cell_info(out_point).map(|ci| ci.data.to_owned())
    }

    fn get_cell_data_hash(&self, out_point: &OutPoint) -> Option<Byte32> {
        self.get_cell_info(out_point)
            .map(|ci| ci.data_hash.to_owned())
    }
}

impl HeaderProvider for TxDataLoader {
    fn get_header(&self, block_hash: &Byte32) -> Option<HeaderView> {
        self.headers.get(block_hash).cloned()
    }
}

#[derive(Clone)]
struct CellInfo {
    output: CellOutput,
    data: Bytes,
    data_hash: Byte32,
}

fn into_info(input_cell_info: gw_types::offchain::InputCellInfo) -> (OutPoint, CellInfo) {
    let out_point = OutPoint::new_unchecked(input_cell_info.cell.out_point.as_bytes());
    let cell_info = CellInfo {
        output: CellOutput::new_unchecked(input_cell_info.cell.output.as_bytes()),
        data_hash: CellOutput::calc_data_hash(&input_cell_info.cell.data),
        data: input_cell_info.cell.data,
    };

    (out_point, cell_info)
}
