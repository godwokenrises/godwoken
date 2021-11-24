use anyhow::anyhow;
use bytes::Bytes;
use std::{
    cmp,
    time::{Duration, Instant},
};

use ckb_fixed_hash::H256;

use anyhow::Result;
use ckb_jsonrpc_types::JsonBytes;
use gw_tools::types::ScriptsDeploymentResult;
use rand::prelude::*;
use reqwest::Url;
use tokio::{sync::mpsc, time};

use crate::{
    batch::BatchResMsg, godwoken_rpc::GodwokenRpcClient, stats::StatsHandler, tx::TxMethod,
};

use super::batch::BatchHandler;

pub struct GodwokenConfig {
    pub scripts_deployment: ScriptsDeploymentResult,
    pub url: Url,
    pub rollup_type_hash: H256,
}

pub struct Plan {
    // Send batch requests every {interval}ms
    interval: u64,
    // How many requsts in a batch of sending.
    req_batch_cnt: usize,
    batch_handler: BatchHandler,
    batch_res_receiver: mpsc::Receiver<BatchResMsg>,
    accounts: Vec<Account>,
    rng: ThreadRng,
    time_to_stop: Option<u64>,
    stats_handler: StatsHandler,
}

impl Plan {
    pub(crate) async fn new(
        interval: u64,
        time_to_stop: Option<u64>,
        accounts: Vec<Account>,
        req_batch_cnt: usize,
        batch_handler: BatchHandler,
        batch_res_receiver: mpsc::Receiver<BatchResMsg>,
        stats_handler: StatsHandler,
    ) -> Result<Self> {
        Ok(Self {
            interval,
            time_to_stop,
            accounts,
            req_batch_cnt,
            batch_handler,
            stats_handler,
            batch_res_receiver,
            rng: rand::thread_rng(),
        })
    }

    pub(crate) async fn run(&mut self) {
        log::info!("Plan running...");
        let tick = Instant::now();
        let req_freq = Duration::from_millis(self.interval);
        let mut interval = time::interval(req_freq);
        let mut wait_pk_interval = time::interval(Duration::from_secs(5));

        loop {
            if let Some(time_to_stop) = &self.time_to_stop {
                if *time_to_stop > tick.elapsed().as_secs() {
                    log::info!("Last stats: {:?}", self.stats_handler.get_stats().await);
                    break;
                }
            }
            if let Some(pks) = self.next_batch() {
                log::debug!("run next batch: {} requests", pks.len());
                let batch_handler = self.batch_handler.clone();
                batch_handler.send_batch(pks, TxMethod::Submit, 1_000).await;
            } else {
                log::warn!("All privkeys are used in txs!");
                wait_pk_interval.tick().await;
            }

            if let Ok(msg) = self.batch_res_receiver.try_recv() {
                log::debug!("receive batch responses: {}", &msg.pk_idx_vec.len());
                for pk_idx in msg.pk_idx_vec {
                    if let Some(account) = self.accounts.get_mut(pk_idx) {
                        account.available = Some(())
                    }
                }
            }
            interval.tick().await;
        }
    }

    fn next_batch(&mut self) -> Option<Vec<(Account, usize)>> {
        let mut cnt = 0;
        let mut batch = Vec::new();
        let available_idx_vec: Vec<usize> = self
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.available.is_some())
            .map(|(idx, _)| idx)
            .collect();
        if available_idx_vec.is_empty() {
            return None;
        }
        log::debug!("available: {}", available_idx_vec.len());
        let batch_cnt = cmp::min(available_idx_vec.len(), self.req_batch_cnt);
        loop {
            let nxt = self.rng.gen_range(0..available_idx_vec.len());
            let idx = available_idx_vec.get(nxt).unwrap();
            if let Some(a) = self.accounts.get_mut(*idx) {
                if a.available.is_some() {
                    a.available = None;
                    batch.push((a.clone(), *idx));
                    cnt += 1;
                    if batch_cnt == cnt {
                        break;
                    }
                }
            }
        }
        Some(batch)
    }
}

pub(crate) async fn to_account(
    pk: H256,
    rpc_client: &mut GodwokenRpcClient,
    scripts_deployment: &ScriptsDeploymentResult,
    rollup_type_hash: &H256,
) -> Result<Account> {
    log::debug!("pk: {}", hex::encode(&pk));
    let short_address =
        gw_tools::account::privkey_to_short_address(&pk, rollup_type_hash, scripts_deployment)?;
    log::debug!("short addr: {}", hex::encode(&short_address));
    let account_id = short_address_to_account_id(rpc_client, &short_address)
        .await?
        .ok_or_else(|| anyhow!("No account"))?;
    log::debug!("account id: {}", account_id);
    let short_address = JsonBytes::from_bytes(short_address);
    let balance = rpc_client.get_balance(short_address, 1).await?;
    log::debug!("accout: {}, balance: {}", account_id, balance);
    Ok(Account {
        pk,
        account_id,
        available: Some(()),
        balance,
    })
}

pub(crate) async fn get_accounts(
    pks: Vec<H256>,
    gw_config: &GodwokenConfig,
) -> Result<Vec<Account>> {
    let mut accounts = Vec::new();
    for pk in pks.into_iter() {
        let mut rpc_client = GodwokenRpcClient::new(gw_config.url.clone());
        let account = to_account(
            pk,
            &mut rpc_client,
            &gw_config.scripts_deployment,
            &gw_config.rollup_type_hash,
        )
        .await;
        accounts.push(account?);
    }
    log::info!("Valid accounts: {}", &accounts.len());
    Ok(accounts)
}

#[derive(Clone, Debug)]
pub(crate) struct Account {
    pub(crate) pk: H256,
    pub(crate) account_id: u32,
    pub(crate) available: Option<()>,
    pub(crate) balance: u128,
}

pub async fn short_address_to_account_id(
    godwoken_rpc_client: &mut GodwokenRpcClient,
    short_address: &Bytes,
) -> Result<Option<u32>> {
    let bytes = JsonBytes::from_bytes(short_address.clone());
    let script_hash = match godwoken_rpc_client
        .get_script_hash_by_short_address(bytes)
        .await?
    {
        Some(h) => h,
        None => {
            return Err(anyhow!(
                "script hash by short address: 0x{} not found",
                hex::encode(short_address.to_vec()),
            ))
        }
    };
    let account_id = godwoken_rpc_client
        .get_account_id_by_script_hash(script_hash)
        .await?;

    Ok(account_id)
}
