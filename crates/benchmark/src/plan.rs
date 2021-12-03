use anyhow::anyhow;
use std::{cmp, time::Duration};

use ckb_fixed_hash::H256;

use anyhow::Result;
use ckb_jsonrpc_types::JsonBytes;
use gw_tools::{godwoken_rpc::GodwokenRpcClient, types::ScriptsDeploymentResult};
use rand::prelude::*;
use reqwest::Url;
use tokio::{sync::mpsc, time};

use crate::{batch::BatchResMsg, tx::TxMethod};

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
}

impl Plan {
    pub async fn new(
        interval: u64,
        pks: Vec<H256>,
        gw_config: GodwokenConfig,
        req_batch_cnt: usize,
        batch_handler: BatchHandler,
        batch_res_receiver: mpsc::Receiver<BatchResMsg>,
    ) -> Self {
        let mut rpc_client = GodwokenRpcClient::new(gw_config.url.clone().as_str());
        let accounts: Vec<Account> = pks
            .into_iter()
            .map(|pk| {
                to_account(
                    pk,
                    &mut rpc_client,
                    &gw_config.scripts_deployment,
                    &gw_config.rollup_type_hash,
                )
            })
            .filter(|res| res.is_ok())
            .map(Result::unwrap)
            .collect();

        log::info!("Valid accounts: {}", &accounts.len());
        Self {
            interval,
            accounts,
            req_batch_cnt,
            batch_handler,
            batch_res_receiver,
            rng: rand::thread_rng(),
        }
    }

    pub async fn run(&mut self) {
        log::info!("Plan running...");
        let req_freq = Duration::from_millis(self.interval);
        let mut interval = time::interval(req_freq);
        let mut wait_pk_interval = time::interval(Duration::from_secs(5));

        loop {
            if let Some(pks) = self.next_batch() {
                log::debug!("run next batch: {} requests", pks.len());
                let batch_handler = self.batch_handler.clone();
                batch_handler
                    .send_batch(pks, TxMethod::Submit, 1_000_000_000)
                    .await;
            } else {
                log::warn!("All privkeys are used in txs!");
                wait_pk_interval.tick().await;
            }

            if let Some(msg) = self.batch_res_receiver.recv().await {
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

pub(crate) fn to_account(
    pk: H256,
    rpc_client: &mut GodwokenRpcClient,
    scripts_deployment: &ScriptsDeploymentResult,
    rollup_type_hash: &H256,
) -> Result<Account> {
    let short_address =
        gw_tools::account::privkey_to_short_address(&pk, rollup_type_hash, scripts_deployment)
            .map_err(|err| anyhow!(err))?;
    let account_id = gw_tools::account::short_address_to_account_id(rpc_client, &short_address)
        .map_err(|err| anyhow!(err))?
        .ok_or(anyhow!("No account"))?;
    let short_address = JsonBytes::from_bytes(short_address);
    let balance = rpc_client
        .get_balance(short_address, 1)
        .map_err(|err| anyhow!(err))?;
    Ok(Account {
        pk,
        account_id,
        available: Some(()),
        balance,
    })
}

#[derive(Clone)]
pub(crate) struct Account {
    pub(crate) pk: H256,
    pub(crate) account_id: u32,
    pub(crate) available: Option<()>,
    pub(crate) balance: u128,
}
