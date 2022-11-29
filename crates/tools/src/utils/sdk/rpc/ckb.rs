use ckb_jsonrpc_types::{
    Alert, BannedAddr, Block, BlockEconomicState, BlockNumber, BlockTemplate, BlockView,
    CellWithStatus, ChainInfo, Consensus, EpochNumber, EpochView, ExtraLoggerConfig, HeaderView,
    JsonBytes, LocalNode, MainLoggerConfig, OutPoint, OutputsValidator, RawTxPool, RemoteNode,
    Script, SyncState, Timestamp, Transaction, TransactionProof, TransactionWithStatusResponse,
    TxPoolInfo, Uint64, Version,
};
use ckb_types::H256;

crate::jsonrpc!(pub struct CkbRpcClient {
    // Chain
    pub fn get_block(&mut self, hash: H256) -> Option<BlockView>;
    pub fn get_block_by_number(&mut self, number: BlockNumber) -> Option<BlockView>;
    pub fn get_block_hash(&mut self, number: BlockNumber) -> Option<H256>;
    pub fn get_current_epoch(&mut self) -> EpochView;
    pub fn get_epoch_by_number(&mut self, number: EpochNumber) -> Option<EpochView>;
    pub fn get_header(&mut self, hash: H256) -> Option<HeaderView>;
    pub fn get_header_by_number(&mut self, number: BlockNumber) -> Option<HeaderView>;
    pub fn get_live_cell(&mut self, out_point: OutPoint, with_data: bool) -> CellWithStatus;
    pub fn get_tip_block_number(&mut self) -> BlockNumber;
    pub fn get_tip_header(&mut self) -> HeaderView;
    pub fn get_transaction(&mut self, hash: H256) -> Option<TransactionWithStatusResponse>;
    pub fn get_transaction_proof(
        &mut self,
        tx_hashes: Vec<H256>,
        block_hash: Option<H256>
    ) -> TransactionProof;
    pub fn verify_transaction_proof(&mut self, tx_proof: TransactionProof) -> Vec<H256>;
    pub fn get_fork_block(&mut self, block_hash: H256) -> Option<BlockView>;
    pub fn get_consensus(&mut self) -> Consensus;
    pub fn get_block_median_time(&mut self, block_hash: H256) -> Option<Timestamp>;
    pub fn get_block_economic_state(&mut self, block_hash: H256) -> Option<BlockEconomicState>;

    // Net
    pub fn get_banned_addresses(&mut self) -> Vec<BannedAddr>;
    pub fn get_peers(&mut self) -> Vec<RemoteNode>;
    pub fn local_node_info(&mut self) -> LocalNode;
    pub fn set_ban(
        &mut self,
        address: String,
        command: String,
        ban_time: Option<Timestamp>,
        absolute: Option<bool>,
        reason: Option<String>
    ) -> ();
    pub fn sync_state(&mut self) -> SyncState;
    pub fn set_network_active(&mut self, state: bool) -> ();
    pub fn add_node(&mut self, peer_id: String, address: String) -> ();
    pub fn remove_node(&mut self, peer_id: String) -> ();
    pub fn clear_banned_addresses(&mut self) -> ();
    pub fn ping_peers(&mut self) -> ();

    // Pool
    pub fn send_transaction(&mut self, tx: Transaction, outputs_validator: Option<OutputsValidator>) -> H256;
    pub fn remove_transaction(&mut self, tx_hash: H256) -> bool;
    pub fn tx_pool_info(&mut self) -> TxPoolInfo;
    pub fn clear_tx_pool(&mut self) -> ();
    pub fn get_raw_tx_pool(&mut self, verbose: Option<bool>) -> RawTxPool;
    pub fn tx_pool_ready(&mut self) -> bool;

    // Stats
    pub fn get_blockchain_info(&mut self) -> ChainInfo;

    // Miner
    pub fn get_block_template(&mut self, bytes_limit: Option<Uint64>, proposals_limit: Option<Uint64>, max_version: Option<Version>) -> BlockTemplate;
    pub fn submit_block(&mut self, _work_id: String, _data: Block) -> H256;

    // Alert
    pub fn send_alert(&mut self, alert: Alert) -> ();

    // IntegrationTest
    pub fn process_block_without_verify(&mut self, data: Block, broadcast: bool) -> Option<H256>;
    pub fn truncate(&mut self, target_tip_hash: H256) -> ();
    pub fn generate_block(&mut self, block_assembler_script: Option<Script>, block_assembler_message: Option<JsonBytes>) -> H256;
    pub fn notify_transaction(&mut self, tx: Transaction) -> H256;

    // Debug
    pub fn jemalloc_profiling_dump(&mut self) -> String;
    pub fn update_main_logger(&mut self, config: MainLoggerConfig) -> ();
    pub fn set_extra_logger(&mut self, name: String, config_opt: Option<ExtraLoggerConfig>) -> ();
});
