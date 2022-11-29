use serde::{Deserialize, Serialize};

use ckb_jsonrpc_types::{
    BlockNumber, BlockView, HeaderView, JsonBytes, NodeAddress, RemoteNodeProtocol, Script,
    Transaction, TransactionView, Uint32, Uint64,
};
use ckb_types::H256;

pub use crate::utils::sdk::rpc::ckb_indexer::{
    Cell, CellType, CellsCapacity, Order, Pagination, ScriptType, SearchKey, SearchKeyFilter,
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScriptStatus {
    pub script: Script,
    pub script_type: ScriptType,
    pub block_number: BlockNumber,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug)]
#[serde(tag = "status")]
#[serde(rename_all = "snake_case")]
pub enum FetchStatus<T> {
    Added { timestamp: Uint64 },
    Fetching { first_sent: Uint64 },
    Fetched { data: T },
    NotFound,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct TransactionWithHeader {
    pub transaction: TransactionView,
    pub header: HeaderView,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Tx {
    Ungrouped(TxWithCell),
    Grouped(TxWithCells),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCell {
    transaction: TransactionView,
    block_number: BlockNumber,
    tx_index: Uint32,
    io_index: Uint32,
    io_type: CellType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCells {
    transaction: TransactionView,
    block_number: BlockNumber,
    tx_index: Uint32,
    cells: Vec<(CellType, Uint32)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemoteNode {
    /// The remote node version.
    pub version: String,
    /// The remote node ID which is derived from its P2P private key.
    pub node_id: String,
    /// The remote node addresses.
    pub addresses: Vec<NodeAddress>,
    /// Elapsed time in milliseconds since the remote node is connected.
    pub connected_duration: Uint64,
    /// Null means chain sync has not started with this remote node yet.
    pub sync_state: Option<PeerSyncState>,
    /// Active protocols.
    ///
    /// CKB uses Tentacle multiplexed network framework. Multiple protocols are running
    /// simultaneously in the connection.
    pub protocols: Vec<RemoteNodeProtocol>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeerSyncState {
    /// Requested best known header of remote peer.
    ///
    /// This is the best known header yet to be proved.
    pub requested_best_known_header: Option<HeaderView>,
    /// Proved best known header of remote peer.
    pub proved_best_known_header: Option<HeaderView>,
}

crate::jsonrpc!(pub struct LightClientRpcClient {
    // BlockFilter
    pub fn set_scripts(&mut self, scripts: Vec<ScriptStatus>) -> ();
    pub fn get_scripts(&mut self) -> Vec<ScriptStatus>;
    pub fn get_cells(&mut self, search_key: SearchKey, order: Order, limit: Uint32, after: Option<JsonBytes>) -> Pagination<Cell>;
    pub fn get_transactions(&mut self, search_key: SearchKey, order: Order, limit: Uint32, after: Option<JsonBytes>) -> Pagination<Tx>;
    pub fn get_cells_capacity(&mut self, search_key: SearchKey) -> CellsCapacity;

    // Transaction
    pub fn send_transaction(&mut self, tx: Transaction) -> H256;

    // Chain
    pub fn get_tip_header(&mut self) -> HeaderView;
    pub fn get_genesis_block(&mut self) -> BlockView;
    pub fn get_header(&mut self, block_hash: H256) -> Option<HeaderView>;
    pub fn get_transaction(&mut self, tx_hash: H256) -> Option<TransactionWithHeader>;
    /// Fetch a header from remote node. If return status is `not_found` will re-sent fetching request immediately.
    ///
    /// Returns: FetchStatus<HeaderView>
    pub fn fetch_header(&mut self, block_hash: H256) -> FetchStatus<HeaderView>;

    /// Fetch a transaction from remote node. If return status is `not_found` will re-sent fetching request immediately.
    ///
    /// Returns: FetchStatus<TransactionWithHeader>
    pub fn fetch_transaction(&mut self, tx_hash: H256) -> FetchStatus<TransactionWithHeader>;

    // Net
    pub fn get_peers(&mut self) -> Vec<RemoteNode>;
});
