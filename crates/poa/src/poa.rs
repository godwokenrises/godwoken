//! POA lock off-chain module
//! Reference implementation: https://github.com/nervosnetwork/clerkb/blob/main/src/generator.ts

// use crate::transaction_skeleton::TransactionSkeleton;
use anyhow::{anyhow, Result};
use gw_common::H256;
use gw_rpc_client::RPCClient;
use gw_types::{
    bytes::Bytes,
    core::DepType,
    offchain::{CellInfo, InputCellInfo},
    packed::{CellDep, CellInput, CellOutput, PoAData, Script},
    prelude::*,
};
use std::convert::TryInto;
use std::time::Duration;

/// Transaction since flag
const SINCE_BLOCK_TIMESTAMP_FLAG: u64 = 0x4000_0000_0000_0000;

pub struct PoASetup {
    pub identity_size: u8,
    pub round_interval_uses_seconds: bool,
    pub identities: Vec<Vec<u8>>,
    pub block_producers_change_threshold: u8,
    pub round_intervals: u32,
    pub subblocks_per_round: u32,
}

impl PoASetup {
    const MAX_IDENTITY_SIZE: u8 = 32;

    fn from_slice(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(anyhow!("invalid POASetup"));
        }
        let round_interval_uses_seconds = data[0] == 1;
        let identity_size = data[1];
        let block_producer_number = data[2];
        let block_producers_change_threshold = data[3];
        let round_intervals = {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(&data[4..8]);
            u32::from_le_bytes(buf)
        };
        let subblocks_per_round = {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(&data[8..12]);
            u32::from_le_bytes(buf)
        };
        if identity_size > Self::MAX_IDENTITY_SIZE {
            return Err(anyhow!(
                "invalid identity size, max: {} got: {}",
                Self::MAX_IDENTITY_SIZE,
                identity_size
            ));
        }
        if block_producers_change_threshold > block_producer_number {
            return Err(anyhow!(
                "invalid block producer change threshold, block_producer_number: {}, threshold: {}",
                block_producer_number,
                block_producers_change_threshold
            ));
        }
        if data.len() != 12 + identity_size as usize * block_producer_number as usize {
            return Err(anyhow!("PoA data has invalid length"));
        }

        let identities = (0..block_producer_number as usize)
            .map(|i| {
                let start = 12 + identity_size as usize * i;
                let end = start + identity_size as usize;
                data[start..end].to_vec()
            })
            .collect();

        let poa_setup = PoASetup {
            identity_size,
            round_interval_uses_seconds,
            identities,
            block_producers_change_threshold,
            round_intervals,
            subblocks_per_round,
        };
        assert!(poa_setup.block_producers_change_threshold > 0);
        Ok(poa_setup)
    }
}

pub(crate) struct PoAContext {
    pub poa_data: PoAData,
    pub poa_data_cell: CellInfo,
    pub poa_setup: PoASetup,
    pub poa_setup_cell: CellInfo,
    pub block_producer_index: u16,
}

pub struct PoA {
    client: RPCClient,
    owner_lock: Script,
    lock_cell_dep: CellDep,
    state_cell_dep: CellDep,
    round_start_subtime: Option<Duration>,
}

#[derive(PartialEq, Eq)]
pub enum ShouldIssueBlock {
    Yes,
    YesIfFull,
    No,
}

impl PoA {
    pub fn new(
        client: RPCClient,
        owner_lock: Script,
        lock_cell_dep: CellDep,
        state_cell_dep: CellDep,
    ) -> Self {
        PoA {
            client,
            owner_lock,
            lock_cell_dep,
            state_cell_dep,
            round_start_subtime: None,
        }
    }

    async fn query_poa_state_cell(&self, type_hash: H256) -> Result<Option<CellInfo>> {
        let args = type_hash.as_slice().to_vec().into();
        let cell = self.client.query_identity_cell(args).await?;
        Ok(cell)
    }

    pub(crate) async fn query_poa_context(&self, input_info: &InputCellInfo) -> Result<PoAContext> {
        let args: Bytes = input_info.cell.output.lock().args().unpack();
        if args.len() != 64 {
            return Err(anyhow!("invalid poa cell lock args"));
        }
        let poa_setup_cell_type_hash: H256 = {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&args[..32]);
            hash.into()
        };
        let poa_data_cell_type_hash: H256 = {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&args[32..]);
            hash.into()
        };
        let poa_data_cell = self
            .query_poa_state_cell(poa_data_cell_type_hash)
            .await?
            .ok_or_else(|| anyhow!("can't find poa data cell"))?;
        let poa_data = PoAData::from_slice(&poa_data_cell.data)?;

        let poa_setup_cell = self
            .query_poa_state_cell(poa_setup_cell_type_hash)
            .await?
            .ok_or_else(|| anyhow!("can't find poa setup cell"))?;
        let poa_setup = PoASetup::from_slice(&poa_setup_cell.data)?;
        if !poa_setup.round_interval_uses_seconds {
            return Err(anyhow!("Block interval PoA is unimplemented yet"));
        }
        let truncated_script_hash = {
            let script_hash = self.owner_lock_hash();
            if poa_setup.identity_size > 32 {
                return Err(anyhow!(
                    "invalid identify_size: {}",
                    poa_setup.identity_size
                ));
            }
            script_hash.as_slice()[..poa_setup.identity_size as usize].to_vec()
        };
        let block_producer_index = poa_setup
            .identities
            .iter()
            .enumerate()
            .find_map(|(index, identity)| {
                if &truncated_script_hash == identity {
                    Some(index)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("can't find current block producer in the PoA identities"))?
            .try_into()?;
        Ok(PoAContext {
            poa_data,
            poa_data_cell,
            poa_setup,
            poa_setup_cell,
            block_producer_index,
        })
    }

    fn owner_lock_hash(&self) -> H256 {
        self.owner_lock.hash().into()
    }

    pub fn estimate_next_round_start_time(&self, ctx: PoAContext) -> Duration {
        let PoAContext {
            poa_data,
            poa_setup,
            block_producer_index,
            ..
        } = ctx;
        // calculate the steps to next round for us
        let identities_len = poa_setup.identities.len() as u64;
        let mut steps = (block_producer_index as u64)
            .saturating_add(identities_len)
            .saturating_sub({
                let index: u16 = poa_data.block_producer_index().unpack();
                index as u64
            })
            % identities_len;
        if steps == 0 {
            steps = identities_len;
        }

        let initial_time: u64 = poa_data.round_initial_subtime().unpack();
        let seconds = initial_time + poa_setup.round_intervals as u64 * steps;
        Duration::from_secs(seconds)
    }

    pub async fn should_issue_next_block(
        &mut self,
        median_time: Duration,
        poa_cell_input: &InputCellInfo,
    ) -> Result<ShouldIssueBlock> {
        let poa_ctx = self.query_poa_context(poa_cell_input).await?;

        if let Some(round_start_subtime) = self.round_start_subtime {
            let next_round_time = round_start_subtime
                .as_secs()
                .saturating_add(poa_ctx.poa_setup.round_intervals.try_into()?);
            if next_round_time > median_time.as_secs() {
                // within current block produce round
                return Ok(ShouldIssueBlock::YesIfFull);
            } else {
                // reset current round
                self.round_start_subtime = None;
            }
        }

        let next_start_time = self.estimate_next_round_start_time(poa_ctx);

        // check next start time again
        if next_start_time <= median_time {
            self.round_start_subtime = Some(median_time);
            return Ok(ShouldIssueBlock::Yes);
        }
        Ok(ShouldIssueBlock::No)
    }

    pub fn reset_current_round(&mut self) {
        self.round_start_subtime = None;
    }

    pub async fn generate(
        &self,
        poa_cell_input: &InputCellInfo,
        inputs: &[InputCellInfo],
        median_time: Duration,
    ) -> Result<GeneratedPoA> {
        let PoAContext {
            poa_data,
            poa_data_cell,
            poa_setup,
            poa_setup_cell,
            block_producer_index,
        } = self.query_poa_context(poa_cell_input).await?;

        let mut cell_deps = Vec::new();

        // put cell deps
        cell_deps.push(self.lock_cell_dep.clone());
        cell_deps.push(self.state_cell_dep.clone());
        // push PoA setup cell to dep
        cell_deps.push(
            CellDep::new_builder()
                .out_point(poa_setup_cell.out_point)
                .dep_type(DepType::Code.into())
                .build(),
        );

        let mut input_cells = Vec::new();

        // push PoA data cell
        input_cells.push(InputCellInfo {
            input: CellInput::new_builder()
                .previous_output(poa_data_cell.out_point.clone())
                .build(),
            cell: poa_data_cell.clone(),
        });

        // new PoA data
        let new_poa_data = {
            let data_round_initial_subtime: u64 = poa_data.round_initial_subtime().unpack();
            let data_subblock_index: u32 = poa_data.subblock_index().unpack();
            let data_subblock_subtime: u64 = poa_data.subblock_subtime().unpack();
            let data_block_producer_index = poa_data.block_producer_index();
            if median_time.as_secs() < data_round_initial_subtime + poa_setup.round_intervals as u64
                && data_subblock_index + 1 < poa_setup.subblocks_per_round
            {
                PoAData::new_builder()
                    .round_initial_subtime(data_round_initial_subtime.pack())
                    .subblock_subtime((data_subblock_subtime + 1).pack())
                    .subblock_index((data_subblock_index + 1).pack())
                    .block_producer_index(data_block_producer_index)
                    .build()
            } else {
                PoAData::new_builder()
                    .round_initial_subtime(median_time.as_secs().pack())
                    .subblock_subtime(median_time.as_secs().pack())
                    .subblock_index(0u32.pack())
                    .block_producer_index(block_producer_index.pack())
                    .build()
            }
        };

        // Update PoA cell since time
        // TODO: block interval handling
        let poa_input_cell_since =
            SINCE_BLOCK_TIMESTAMP_FLAG | new_poa_data.subblock_subtime().unpack();

        let mut output_cells = Vec::new();
        output_cells.push((poa_data_cell.output, new_poa_data.as_bytes()));

        // Push owner cell if not exists
        let mut owner_input_cell = None;
        let exists_owner_cell = inputs.iter().any(|input_info| {
            let lock_hash: H256 = input_info.cell.output.lock().hash().into();
            lock_hash == self.owner_lock_hash()
        });
        if !exists_owner_cell {
            let owner_cell = self
                .client
                .query_owner_cell(self.owner_lock.clone(), None)
                .await?
                .ok_or_else(|| anyhow!("can't find usable owner cell"))?;
            // put owner cell to input, the change cell will complete the output
            owner_input_cell = Some(InputCellInfo {
                input: CellInput::new_builder()
                    .previous_output(owner_cell.out_point.clone())
                    .build(),
                cell: owner_cell,
            });
        }

        let poa = GeneratedPoA {
            poa_input_cell_since,
            owner_input_cell,
            input_cells,
            output_cells,
            cell_deps,
        };

        Ok(poa)
    }
}

pub struct GeneratedPoA {
    pub poa_input_cell_since: u64,
    pub owner_input_cell: Option<InputCellInfo>,
    pub input_cells: Vec<InputCellInfo>,
    pub output_cells: Vec<(CellOutput, Bytes)>,
    pub cell_deps: Vec<CellDep>,
}
