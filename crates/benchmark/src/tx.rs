use anyhow::{anyhow, Result};
use bytes::Bytes;
use gw_tools::utils::message::generate_transaction_message_to_sign;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::JsonBytes;
use ckb_types::prelude::Builder;
use gw_jsonrpc_types::godwoken::L2TransactionStatus;
use gw_tools::godwoken_rpc::GodwokenRpcClient;
use gw_types::packed::{L2Transaction, RawL2Transaction};
use gw_types::{prelude::Entity as GwEntity, prelude::Pack as GwPack};
use reqwest::Url;
use tokio::sync::{mpsc, oneshot};

use crate::polyman::{self, BuildDeployResponse, BuildErc20Response, PolymanClient};
use crate::stats::{ApiStatus, StatsHandler};

const API_SUBMIT_TX: &str = "submit_tx";

pub enum TxStatus {
    Failure,
    PendingCommit,
    Committed,
    Timeout,
}

#[derive(Clone, Copy)]
pub enum TxMethod {
    Submit,
    Execute,
}

pub(crate) struct TxMsg {
    pub(crate) pk_from: H256,
    pub(crate) from_id: u32,
    pub(crate) to_id: u32,
    pub(crate) args: Vec<u8>,
    pub(crate) method: TxMethod,
    pub(crate) callback: oneshot::Sender<()>,
    receiver_script_hash: H256,
}

impl TxMsg {
    fn new_submit(
        pk_from: H256,
        from_id: u32,
        to_id: u32,
        args: Vec<u8>,
        receiver_script_hash: H256,
        callback: oneshot::Sender<()>,
    ) -> Self {
        Self {
            pk_from,
            from_id,
            to_id,
            args,
            receiver_script_hash,
            callback,
            method: TxMethod::Submit,
        }
    }
    #[allow(dead_code)]
    fn new_execute(
        pk_from: H256,
        from_id: u32,
        to_id: u32,
        args: Vec<u8>,
        receiver_script_hash: H256,
        callback: oneshot::Sender<()>,
    ) -> Self {
        Self {
            pk_from,
            from_id,
            to_id,
            args,
            receiver_script_hash,
            callback,
            method: TxMethod::Execute,
        }
    }

    fn build_tx(
        &self,
        rpc_client: &mut GodwokenRpcClient,
        rollup_type_hash: &H256,
    ) -> Result<L2Transaction> {
        let nonce = rpc_client
            .get_nonce(self.from_id)
            .map_err(|err| anyhow!(err))?;
        let raw_l2transaction = RawL2Transaction::new_builder()
            .from_id(GwPack::pack(&self.from_id))
            .to_id(GwPack::pack(&self.to_id))
            .nonce(GwPack::pack(&nonce))
            .args(GwPack::pack(&Bytes::from(self.args.clone())))
            .build();

        let sender_script_hash = rpc_client
            .get_script_hash(self.from_id)
            .map_err(|err| anyhow!(err))?;

        let message = generate_transaction_message_to_sign(
            &raw_l2transaction,
            rollup_type_hash,
            &sender_script_hash,
            &self.receiver_script_hash,
        );
        let signature = gw_tools::account::eth_sign(&message, self.pk_from.clone())
            .map_err(|err| anyhow!(err))?;

        let l2_tx = L2Transaction::new_builder()
            .raw(raw_l2transaction)
            .signature(signature.pack())
            .build();

        Ok(l2_tx)
    }
}
struct TxActor {
    url: Url,
    receiver: mpsc::Receiver<TxMsg>,
    rollup_type_hash: H256,
    timeout: u64,
    stats_handler: StatsHandler,
}

impl TxActor {
    fn new(
        timeout: u64,
        url: Url,
        rollup_type_hash: H256,
        receiver: mpsc::Receiver<TxMsg>,
        stats_handler: StatsHandler,
    ) -> Self {
        Self {
            url,
            receiver,
            rollup_type_hash,
            timeout,
            stats_handler,
        }
    }
    fn handle_msg(&self, msg: TxMsg) {
        if let TxMethod::Submit = msg.method {
            self.handle_submit_msg(msg)
        }
    }

    fn handle_submit_msg(&self, tx_msg: TxMsg) {
        let rollup_type_hash = self.rollup_type_hash.clone();
        let url = self.url.clone();
        let timeout = self.timeout;
        let stats_handler = self.stats_handler.clone();
        tokio::spawn(async move {
            let mut rpc_client = GodwokenRpcClient::new(url.as_str());
            let timer = Instant::now();
            let tx = match tx_msg
                .build_tx(&mut rpc_client, &rollup_type_hash)
                .and_then(|tx| {
                    let bytes = JsonBytes::from_bytes(tx.as_bytes());
                    rpc_client
                        .submit_l2transaction(bytes)
                        .map_err(|err| anyhow!(err))
                }) {
                Ok(tx) => {
                    log::debug!("submit tx: {}", hex::encode(&tx));
                    let _ = stats_handler
                        .send_api_stats(API_SUBMIT_TX.into(), timer.elapsed(), ApiStatus::Success)
                        .await;
                    tx
                }
                Err(err) => {
                    log::error!("submit l2 tx with error: {:?}", err);
                    let _ = stats_handler.send_tx_stats(TxStatus::Failure).await;
                    let _ = stats_handler
                        .send_api_stats(API_SUBMIT_TX.into(), timer.elapsed(), ApiStatus::Failure)
                        .await;
                    let _ = tx_msg.callback.send(());
                    return;
                }
            };
            if let Ok(_) = wait_receipt(&tx, &mut rpc_client, timeout).await {
                let _ = stats_handler.send_tx_stats(TxStatus::PendingCommit).await;
            } else {
                let _ = stats_handler.send_tx_stats(TxStatus::Timeout).await;
                let _ = tx_msg.callback.send(());
                return;
            }
            spawn_wait_committed_task(tx, stats_handler.clone(), rpc_client, timeout);
            let _ = tx_msg.callback.send(());
        });
    }
}

async fn transfer_handler(mut actor: TxActor) {
    log::info!("transfer handler is running now");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_msg(msg);
    }
}

#[derive(Clone)]
pub struct TxHandler {
    sender: Sender<TxMsg>,
    proxy_contract_id: u32,
    polyman_client: PolymanClient,
    proxy_contract_script_hash: H256,
}

impl TxHandler {
    pub async fn new(
        timeout: u64,
        gw_url: Url,
        polyman_url: Url,
        rollup_type_hash: H256,
        stats_handler: StatsHandler,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel(200);

        let actor = TxActor::new(timeout, gw_url, rollup_type_hash, receiver, stats_handler);

        tokio::spawn(transfer_handler(actor));

        let polyman_client = PolymanClient::new(polyman_url);
        let res = polyman_client.deploy().await?;
        if let polyman::Status::Failed = res.status {
            return Err(anyhow!("Deploy erc20 contract failed: {:?}", &res.error));
        }
        let BuildDeployResponse {
            proxy_contract_id,
            proxy_contract_script_hash,
        } = res.data.unwrap();
        Ok(Self {
            sender,
            proxy_contract_id: proxy_contract_id.value(),
            proxy_contract_script_hash,
            polyman_client,
        })
    }

    pub async fn submit_erc20_tx(
        &self,
        pk_from: H256,
        from_id: u32,
        to_id: u32,
        amount: u128,
    ) -> Result<()> {
        let res = self
            .polyman_client
            .build_erc20(from_id, to_id, amount)
            .await?;
        if let polyman::Status::Failed = res.status {
            return Err(anyhow!("build erc20 tx req failed: {:?}", &res.error));
        }
        let BuildErc20Response { nonce: _, args } = res.data.unwrap();
        let args = hex::decode(&args.trim_start_matches("0x"))?;
        let (callback, recv) = oneshot::channel();
        let msg = TxMsg::new_submit(
            pk_from,
            from_id,
            to_id,
            args,
            self.proxy_contract_script_hash.clone(),
            callback,
        );
        let _ = self.sender.send(msg).await;
        let _ = recv.await;
        Ok(())
    }
}
async fn wait_receipt(tx: &H256, rpc_client: &mut GodwokenRpcClient, timeout: u64) -> Result<()> {
    let ts = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        if let Ok(res) = rpc_client.get_transaction_receipt(tx) {
            match res {
                Some(_) => {
                    log::debug!("pending commit tx: {}", hex::encode(tx));
                    return Ok(());
                }
                None => {
                    if ts.elapsed().as_secs() > timeout {
                        return Err(anyhow!("Wait receipt timeout"));
                    }
                }
            }
        }
    }
}

async fn wait_committed(tx: &H256, rpc_client: &mut GodwokenRpcClient, timeout: u64) -> Result<()> {
    let ts = Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        if let Ok(Some(tx_status)) = rpc_client.get_transaction(tx) {
            if tx_status.status == L2TransactionStatus::Committed {
                log::debug!("committed tx: {}", hex::encode(tx));
                return Ok(());
            }
        }
        if ts.elapsed().as_secs() > timeout {
            return Err(anyhow!("Wait committed timeout"));
        }
    }
}

fn spawn_wait_committed_task(
    tx: H256,
    stats_handler: StatsHandler,
    mut rpc_client: GodwokenRpcClient,
    timeout: u64,
) {
    tokio::spawn(async move {
        match wait_committed(&tx, &mut rpc_client, timeout).await {
            Ok(_) => {
                let _ = stats_handler.send_tx_stats(TxStatus::Committed).await;
            }
            Err(_) => {
                let _ = stats_handler.send_tx_stats(TxStatus::Timeout).await;
            }
        };
    });
}
