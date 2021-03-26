use crate::indexer_types::{Order, Pagination, ScriptType, SearchKey, SearchKeyFilter, Tx};
use async_jsonrpc_client::{HttpClient, Output, Params as ClientParams, Transport};
use ckb_fixed_hash::H256;
use gw_chain::chain::{Chain, L1Action, L1ActionContext, SyncParam};
use gw_generator::RollupContext;
use gw_jsonrpc_types::ckb_jsonrpc_types::{BlockNumber, HeaderView, TransactionWithStatus, Uint32};
use gw_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{
        Byte32, CellOutput, DepositionLockArgs, DepositionRequest, HeaderInfo, L2Block, Script,
        Transaction, WitnessArgs,
    },
    prelude::*,
};
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde_json::{from_value, json};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::sync::Arc;

fn to_result<T: DeserializeOwned>(output: Output) -> anyhow::Result<T> {
    match output {
        Output::Success(success) => Ok(from_value(success.result)?),
        Output::Failure(failure) => Err(anyhow::anyhow!("JSONRPC error: {}", failure.error)),
    }
}

pub async fn poll_loop(
    chain: Arc<RwLock<Chain>>,
    rollup_context: &RollupContext,
    rollup_type_script: Script,
    ckb_rpc: String,
    indexer_rpc: String,
    sql_address: String,
) -> anyhow::Result<()> {
    // TODO: support for more SQL databases
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&sql_address)
        .await?;
    let ckb_client = HttpClient::new(&ckb_rpc)?;
    let indexer_client = HttpClient::new(&indexer_rpc)?;
    let mut chain_updater =
        ChainUpdater::new(Arc::clone(&chain), pool, ckb_client, rollup_context.clone());
    let rollup_type_script =
        ckb_types::packed::Script::new_unchecked(rollup_type_script.as_bytes());
    loop {
        let tip_l1_block = chain.read().local_state().last_synced().number();
        let search_key = SearchKey {
            script: rollup_type_script.clone().into(),
            script_type: ScriptType::Type,
            filter: Some(SearchKeyFilter {
                script: None,
                output_data_len_range: None,
                output_capacity_range: None,
                block_range: Some([
                    BlockNumber::from(tip_l1_block.unpack() + 1),
                    BlockNumber::from(u64::max_value()),
                ]),
            }),
        };
        let order = Order::Asc;
        let limit = Uint32::from(1000);

        // TODO: right now this logic does not handle forks well, we will need
        // to tweak this.
        // TODO: the syncing logic here works under the assumption that a single
        // L1 CKB block can contain at most one L2 Godwoken block. The logic
        // here needs revising, once we relax this constraint for more performance.
        let mut txs: Pagination<Tx> = to_result(
            indexer_client
                .request(
                    "get_transactions",
                    Some(ClientParams::Array(vec![
                        json!(search_key),
                        json!(order),
                        json!(limit),
                    ])),
                )
                .await?,
        )?;
        println!("L2 blocks to sync: {}", txs.objects.len());
        chain_updater.update(&txs.objects).await?;

        while txs.objects.len() > 0 {
            txs = to_result(
                indexer_client
                    .request(
                        "get_transactions",
                        Some(ClientParams::Array(vec![
                            json!(search_key),
                            json!(order),
                            json!(limit),
                            json!(txs.last_cursor),
                        ])),
                    )
                    .await?,
            )?;

            println!("L2 blocks to sync: {}", txs.objects.len());
            chain_updater.update(&txs.objects).await?;
        }

        async_std::task::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub struct ChainUpdater {
    pub chain: Arc<RwLock<Chain>>,
    pub ckb_client: HttpClient,
    pub last_tx_hash: Option<H256>,
    pub rollup_context: RollupContext,
    pub pool: PgPool,
}

impl ChainUpdater {
    pub fn new(
        chain: Arc<RwLock<Chain>>,
        pool: PgPool,
        ckb_client: HttpClient,
        rollup_context: RollupContext,
    ) -> ChainUpdater {
        ChainUpdater {
            chain,
            pool,
            ckb_client,
            rollup_context,
            last_tx_hash: None,
        }
    }

    pub async fn update(&mut self, txs: &[Tx]) -> anyhow::Result<()> {
        for tx in txs {
            self.update_single(&tx.tx_hash).await?;
        }
        Ok(())
    }

    async fn update_single(&mut self, tx_hash: &H256) -> anyhow::Result<()> {
        if let Some(last_tx_hash) = &self.last_tx_hash {
            if last_tx_hash == tx_hash {
                return Ok(());
            }
        }
        self.last_tx_hash = Some(tx_hash.clone());

        let tx: Option<TransactionWithStatus> = to_result(
            self.ckb_client
                .request(
                    "get_transaction",
                    Some(ClientParams::Array(vec![json!(tx_hash)])),
                )
                .await?,
        )?;
        let tx_with_status =
            tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
        let tx = {
            let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
            Transaction::new_unchecked(tx.as_bytes())
        };
        let block_hash = tx_with_status.tx_status.block_hash.ok_or_else(|| {
            anyhow::anyhow!("Ttransaction {:x} is not committed on chain!", tx_hash)
        })?;
        let header_view: Option<HeaderView> = to_result(
            self.ckb_client
                .request(
                    "get_header",
                    Some(ClientParams::Array(vec![json!(block_hash)])),
                )
                .await?,
        )?;
        let header_view =
            header_view.ok_or_else(|| anyhow::anyhow!("Cannot locate block: {:x}", block_hash))?;
        let header_info = HeaderInfo::new_builder()
            .number(header_view.inner.number.value().pack())
            .block_hash(block_hash.0.pack())
            .build();
        let requests = self.extract_deposition_requests(&tx).await?;
        let context = L1ActionContext::SubmitTxs {
            deposition_requests: requests,
        };
        let update = L1Action {
            transaction: tx.clone(),
            header_info,
            context,
        };
        // todo handle layer1 fork
        let sync_param = SyncParam {
            reverts: vec![],
            updates: vec![update],
        };
        self.chain.write().sync(sync_param)?;
        self.insert_to_sql(&tx).await?;
        Ok(())
    }

    async fn insert_to_sql(&self, l1_transaction: &Transaction) -> anyhow::Result<()> {
        let witness = l1_transaction
            .witnesses()
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("Witness missing for L2 block!"))?;
        let witness_args = WitnessArgs::from_slice(&witness.raw_data())?;
        let raw_l2_block = witness_args
            .output_type()
            .to_opt()
            .ok_or_else(|| anyhow::anyhow!("Missing L2 block!"))?;
        let l2_block = L2Block::from_slice(&raw_l2_block.raw_data())?;
        let number: u64 = l2_block.raw().number().unpack();
        let hash: H256 = l2_block.raw().hash().into();
        let epoch_time: u64 = l2_block.raw().timestamp().unpack();
        // TODO: this is just a proof of concept work now, we need to fill in more data
        sqlx::query("INSERT INTO blocks (number, hash, parent_hash, logs_bloom, gas_limit, gas_used, timestamp, miner, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(number as i64)
            .bind(format!("{:#x}", hash))
            .bind("0x0000000000000000000000000000000000000000000000000000000000000000")
            .bind("")
            .bind(0i64)
            .bind(0i64)
            .bind(sqlx::types::chrono::NaiveDateTime::from_timestamp(epoch_time as i64, 0))
            .bind(format!("{}", l2_block.raw().block_producer_id()))
            .bind(l2_block.as_slice().len() as i64)
            .execute(&self.pool).await?;
        Ok(())
    }

    async fn extract_deposition_requests(
        &self,
        tx: &Transaction,
    ) -> anyhow::Result<Vec<DepositionRequest>> {
        let mut results = vec![];
        for input in tx.raw().inputs().into_iter() {
            // Load cell denoted by the transaction input
            let tx_hash: H256 = input.previous_output().tx_hash().unpack();
            let index = input.previous_output().index().unpack();
            let tx: Option<TransactionWithStatus> = to_result(
                self.ckb_client
                    .request(
                        "get_transaction",
                        Some(ClientParams::Array(vec![json!(tx_hash)])),
                    )
                    .await?,
            )?;
            let tx_with_status =
                tx.ok_or_else(|| anyhow::anyhow!("Cannot locate transaction: {:x}", tx_hash))?;
            let tx = {
                let tx: ckb_types::packed::Transaction = tx_with_status.transaction.inner.into();
                Transaction::new_unchecked(tx.as_bytes())
            };
            let cell_output = tx
                .raw()
                .outputs()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;
            let cell_data = tx
                .raw()
                .outputs_data()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("OutPoint index out of bound"))?;

            // Check if loaded cell is a deposition request
            if let Some(deposition_request) = try_parse_deposition_request(
                &cell_output,
                &cell_data.unpack(),
                &self.rollup_context,
            ) {
                results.push(deposition_request);
            }
        }
        Ok(results)
    }
}

fn try_parse_deposition_request(
    cell_output: &CellOutput,
    cell_data: &Bytes,
    rollup_context: &RollupContext,
) -> Option<DepositionRequest> {
    if cell_output.lock().code_hash() != rollup_context.rollup_config.deposition_script_type_hash()
        || cell_output.lock().hash_type() != ScriptHashType::Type.into()
    {
        return None;
    }
    let args = cell_output.lock().args().raw_data();
    if args.len() < 32 {
        return None;
    }
    let rollup_type_script_hash: [u8; 32] = rollup_context.rollup_script_hash.into();
    if args.slice(0..32) != &rollup_type_script_hash[..] {
        return None;
    }
    let lock_args = match DepositionLockArgs::from_slice(&args.slice(32..)) {
        Ok(lock_args) => lock_args,
        Err(_) => return None,
    };
    // NOTE: In readoly mode, we are only loading on chain data here, timeout validation
    // can be skipped. For generator part, timeout validation needs to be introduced.
    let (amount, sudt_script) = match cell_output.type_().to_opt() {
        Some(script) => {
            if cell_data.len() < 16 {
                return None;
            }
            let mut data = [0u8; 16];
            data.copy_from_slice(&cell_data[0..16]);
            (u128::from_le_bytes(data), script)
        }
        None => {
            let script = Script::new_builder()
                .code_hash(
                    Byte32::new_builder()
                        .set([gw_types::packed::Byte::new(0); 32])
                        .build(),
                )
                .hash_type(ScriptHashType::Data.into())
                .args(
                    gw_types::packed::Bytes::new_builder()
                        .extend(vec![gw_types::packed::Byte::new(0); 32])
                        .build(),
                )
                .build();
            (0u128, script)
        }
    };
    let capacity: u64 = cell_output.capacity().unpack();
    let sudt_script_hash: [u8; 32] = sudt_script.hash();
    let deposition_request = DepositionRequest::new_builder()
        .capacity(capacity.pack())
        .amount(amount.pack())
        .sudt_script_hash(sudt_script_hash.pack())
        .script(lock_args.layer2_lock())
        .build();
    Some(deposition_request)
}
