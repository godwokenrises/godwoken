use std::{collections::HashSet, path::PathBuf};

use anyhow::{ensure, Result};
use clap::Args;
use gw_rpc_client::{ckb_client::CkbClient, indexer_client::CkbIndexerClient};
use gw_types::{bytes::Bytes, packed, prelude::*};
use gw_utils::{
    fee::{collect_payment_cells, fill_tx_fee_with_local},
    genesis_info::CKBGenesisInfo,
    local_cells::LocalCellsManager,
    transaction_skeleton::TransactionSkeleton,
    type_id::type_id_type_script,
    wallet::Wallet,
};

#[derive(Args)]
pub struct DeployContextArgs {
    #[clap(long, default_value = "http://127.0.0.1:8114")]
    pub ckb_rpc: String,
    #[clap(long)]
    pub ckb_indexer_rpc: Option<String>,
    /// The private key file path
    #[clap(short = 'k', long)]
    pub privkey_path: PathBuf,
}

impl DeployContextArgs {
    pub async fn build(&self) -> Result<DeployContext> {
        let ckb_client = CkbClient::with_url(&self.ckb_rpc)?;
        let ckb_indexer_client = if let Some(ref u) = self.ckb_indexer_rpc {
            CkbIndexerClient::with_url(u)?
        } else {
            CkbIndexerClient::from(ckb_client.clone())
        };
        let wallet = Wallet::from_privkey_path(&self.privkey_path)?;
        let genesis = CKBGenesisInfo::get(&ckb_client).await?;

        Ok(DeployContext {
            ckb_client,
            ckb_indexer_client,
            wallet,
            genesis,
        })
    }
}

pub struct DeployContext {
    pub ckb_client: CkbClient,
    pub ckb_indexer_client: CkbIndexerClient,
    pub wallet: Wallet,
    pub genesis: CKBGenesisInfo,
}

impl DeployContext {
    /// Deploy type id cell.
    ///
    /// Does not wait for the transaction.
    pub async fn deploy_type_id_cell(
        &self,
        lock: packed::Script,
        data: Bytes,
        local_cells: &LocalCellsManager,
    ) -> Result<(packed::Transaction, packed::OutPoint, packed::Script)> {
        let payment_cells = collect_payment_cells(
            &self.ckb_indexer_client,
            self.wallet.lock_script().clone(),
            1,
            &HashSet::new(),
            local_cells,
        )
        .await?;

        ensure!(!payment_cells.is_empty(), "no payment cell");

        let mut tx = TransactionSkeleton::new([0u8; 32]);
        tx.inputs_mut()
            .extend(payment_cells.into_iter().map(Into::into));

        let type_script = type_id_type_script(tx.inputs()[0].input.as_reader(), 0);
        tx.add_output(lock.clone(), Some(type_script.clone()), data)?;

        let tx = self.deploy(tx, local_cells).await?;
        let hash = tx.hash();

        Ok((
            tx,
            packed::OutPoint::new_builder().tx_hash(hash.pack()).build(),
            type_script,
        ))
    }

    /// Add sighash dep, balance, sign and send (but don't wait).
    pub async fn deploy(
        &self,
        mut tx: TransactionSkeleton,
        local_cells: &LocalCellsManager,
    ) -> Result<packed::Transaction> {
        // Sighash dep.
        tx.cell_deps_mut().push(self.genesis.sighash_dep());

        fill_tx_fee_with_local(
            &mut tx,
            &self.ckb_indexer_client,
            self.wallet.lock_script().clone(),
            local_cells,
            1000,
        )
        .await?;

        let tx: packed::Transaction = self.wallet.sign_tx_skeleton(tx)?;
        let ckb_tx = ckb_types::packed::Transaction::new_unchecked(tx.as_bytes());

        self.ckb_client
            .send_transaction(
                ckb_tx.into(),
                Some(ckb_jsonrpc_types::OutputsValidator::Passthrough),
            )
            .await?;

        Ok(tx)
    }
}
