use crate::poa::{PoA, ShouldIssueBlock};
use crate::rpc_client::RPCClient;
use crate::types::InputCellInfo;
use crate::wallet::Wallet;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use ckb_types::prelude::{Builder, Entity};
use gw_common::H256;
use gw_config::{BlockProducerConfig, TestMode};
use gw_jsonrpc_types::{
    godwoken::GlobalState,
    test_mode::{ShouldProduceBlock, TestModePayload},
};
use gw_rpc_server::test_mode_registry::TestModeRPC;
use gw_types::{packed::CellInput, prelude::Unpack};
use smol::lock::Mutex;

use std::sync::Arc;

#[derive(Clone)]
pub struct TestModeControl {
    mode: TestMode,
    payload: Arc<Mutex<Option<TestModePayload>>>,
    rpc_client: RPCClient,
    poa: Arc<Mutex<PoA>>,
}

impl TestModeControl {
    pub fn create(
        mode: TestMode,
        rpc_client: RPCClient,
        config: &BlockProducerConfig,
    ) -> Result<Self> {
        let wallet = Wallet::from_config(&config.wallet_config).with_context(|| "init wallet")?;
        let poa = PoA::new(
            rpc_client.clone(),
            wallet.lock_script().to_owned(),
            config.poa_lock_dep.clone().into(),
            config.poa_state_dep.clone().into(),
        );

        Ok(TestModeControl {
            mode,
            payload: Arc::new(Mutex::new(None)),
            rpc_client,
            poa: Arc::new(Mutex::new(poa)),
        })
    }

    pub fn mode(&self) -> TestMode {
        self.mode
    }

    pub async fn get_payload(&self) -> Option<TestModePayload> {
        self.payload.lock().await.to_owned()
    }

    pub async fn take_payload(&self) -> Option<TestModePayload> {
        self.payload.lock().await.take()
    }
}

#[async_trait]
impl TestModeRPC for TestModeControl {
    async fn get_global_state(&self) -> Result<GlobalState> {
        let rollup_cell = {
            let opt = self.rpc_client.query_rollup_cell().await?;
            opt.ok_or_else(|| anyhow!("rollup cell not found"))?
        };

        let global_state = gw_types::packed::GlobalState::from_slice(&rollup_cell.data)
            .map_err(|_| anyhow!("parse rollup up global state"))?;

        Ok(global_state.into())
    }

    async fn next_global_state(&self, payload: TestModePayload) -> Result<()> {
        *self.payload.lock().await = Some(payload);

        Ok(())
    }

    async fn should_produce_next_block(&self) -> Result<ShouldProduceBlock> {
        let rollup_cell = {
            let opt = self.rpc_client.query_rollup_cell().await?;
            opt.ok_or_else(|| anyhow!("rollup cell not found"))?
        };

        let tip_hash: H256 = {
            let l1_tip_hash_number = self.rpc_client.get_tip().await?;
            let tip_hash: [u8; 32] = l1_tip_hash_number.block_hash().unpack();
            tip_hash.into()
        };

        let ret = {
            let median_time = self.rpc_client.get_block_median_time(tip_hash).await?;
            let poa_cell_input = InputCellInfo {
                input: CellInput::new_builder()
                    .previous_output(rollup_cell.out_point.clone())
                    .build(),
                cell: rollup_cell.clone(),
            };

            let mut poa = self.poa.lock().await;
            poa.should_issue_next_block(median_time, &poa_cell_input)
                .await?
        };

        Ok(match ret {
            ShouldIssueBlock::Yes => ShouldProduceBlock::Yes,
            ShouldIssueBlock::YesIfFull => ShouldProduceBlock::YesIfFull,
            ShouldIssueBlock::No => ShouldProduceBlock::No,
        })
    }
}
