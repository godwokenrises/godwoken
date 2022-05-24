use anyhow::Result;
use ckb_jsonrpc_types::Serialize;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_types::U256;
use std::path::Path;
use tokio::task::JoinHandle;

use crate::godwoken_rpc::GodwokenRpcClient;

#[derive(Serialize, Debug)]
struct Account {
    pub id: u32,
    pub code_hash: ckb_fixed_hash::H256,
    pub ckb: U256,
    pub nonce: u32,
}

type Task = JoinHandle<Result<Option<Account>, anyhow::Error>>;
async fn producer(client: GodwokenRpcClient, tx: tokio::sync::mpsc::Sender<Task>) -> Result<()> {
    for account_id in 0.. {
        let client = client.clone();
        let task = tokio::spawn(async move {
            let script_hash = client.get_script_hash(account_id).await?;
            if script_hash.as_bytes() == [0u8; 32] {
                return Ok::<_, anyhow::Error>(None);
            }
            let script = client
                .get_script(script_hash.clone())
                .await?
                .expect("must hash script");
            let code_hash = script.code_hash;
            // balance
            let addr = client
                .get_registry_address_by_script_hash(&script_hash)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no registry address"))?;
            let ckb = client.get_balance(&addr, CKB_SUDT_ACCOUNT_ID).await?;
            let nonce = client.get_nonce(account_id).await?;
            Ok(Some(Account {
                id: account_id,
                code_hash,
                ckb,
                nonce,
            }))
        });
        tx.send(task).await?;
    }
    Ok(())
}

pub async fn report_accounts<P: AsRef<Path>>(url: &str, output: P) -> Result<()> {
    let client = GodwokenRpcClient::new(url);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Task>(20);
    // start tasks
    let producer_handle = tokio::spawn(producer(client, tx));

    // wait tasks
    let mut total_account = 0;
    let mut wtr = csv::Writer::from_path(output)?;
    while let Some(task) = rx.recv().await {
        match task.await?? {
            Some(account) => {
                wtr.serialize(account)?;
                total_account += 1;
                print!(".")
            }
            None => {
                producer_handle.abort();
                break;
            }
        }
    }
    wtr.flush()?;
    println!("Total accounts: {}", total_account);
    Ok(())
}
