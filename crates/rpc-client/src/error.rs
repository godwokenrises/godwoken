/// Get JSONRPC error code from errors returned by RPC methods.
pub fn get_jsonrpc_error_code(e: &anyhow::Error) -> Option<i64> {
    let e: &jsonrpc_core::types::error::Error = e.downcast_ref()?;
    Some(e.code.code())
}

// Copied from CKB.
pub enum CkbRpcError {
    /// (-1): CKB internal errors are considered to never happen or only happen when the system
    /// resources are exhausted.
    CKBInternalError = -1,
    /// (-2): The CKB method has been deprecated and disabled.
    ///
    /// Set `rpc.enable_deprecated_rpc` to `true` in the config file to enable all deprecated
    /// methods.
    Deprecated = -2,
    /// (-3): Error code -3 is no longer used.
    ///
    /// Before v0.35.0, CKB returns all RPC errors using the error code -3. CKB no longer uses
    /// -3 since v0.35.0.
    Invalid = -3,
    /// (-4): The RPC method is not enabled.
    ///
    /// CKB groups RPC methods into modules, and a method is enabled only when the module is
    /// explicitly enabled in the config file.
    RPCModuleIsDisabled = -4,
    /// (-5): DAO related errors.
    DaoError = -5,
    /// (-6): Integer operation overflow.
    IntegerOverflow = -6,
    /// (-7): The error is caused by a config file option.
    ///
    /// Users have to edit the config file to fix the error.
    ConfigError = -7,
    /// (-101): The CKB local node failed to broadcast a message to its peers.
    P2PFailedToBroadcast = -101,
    /// (-200): Internal database error.
    ///
    /// The CKB node persists data to the database. This is the error from the underlying database
    /// module.
    DatabaseError = -200,
    /// (-201): The chain index is inconsistent.
    ///
    /// An example of an inconsistent index is that the chain index says a block hash is in the chain
    /// but the block cannot be read from the database.
    ///
    /// This is a fatal error usually due to a serious bug. Please back up the data directory and
    /// re-sync the chain from scratch.
    ChainIndexIsInconsistent = -201,
    /// (-202): The underlying database is corrupt.
    ///
    /// This is a fatal error usually caused by the underlying database used by CKB. Please back up
    /// the data directory and re-sync the chain from scratch.
    DatabaseIsCorrupt = -202,
    /// (-301): Failed to resolve the referenced cells and headers used in the transaction, as inputs or
    /// dependencies.
    TransactionFailedToResolve = -301,
    /// (-302): Failed to verify the transaction.
    TransactionFailedToVerify = -302,
    /// (-1000): Some signatures in the submit alert are invalid.
    AlertFailedToVerifySignatures = -1000,
    /// (-1102): The transaction is rejected by the outputs validator specified by the RPC parameter.
    PoolRejectedTransactionByOutputsValidator = -1102,
    /// (-1103): Pool rejects some transactions which seem contain invalid VM instructions. See the issue
    /// link in the error message for details.
    PoolRejectedTransactionByIllTransactionChecker = -1103,
    /// (-1104): The transaction fee rate must be greater than or equal to the config option `tx_pool.min_fee_rate`
    ///
    /// The fee rate is calculated as:
    ///
    /// ```text
    /// fee / (1000 * tx_serialization_size_in_block_in_bytes)
    /// ```
    PoolRejectedTransactionByMinFeeRate = -1104,
    /// (-1105): The in-pool ancestors count must be less than or equal to the config option `tx_pool.max_ancestors_count`
    ///
    /// Pool rejects a large package of chained transactions to avoid certain kinds of DoS attacks.
    PoolRejectedTransactionByMaxAncestorsCountLimit = -1105,
    /// (-1106): The transaction is rejected because the pool has reached its limit.
    PoolIsFull = -1106,
    /// (-1107): The transaction is already in the pool.
    PoolRejectedDuplicatedTransaction = -1107,
    /// (-1108): The transaction is rejected because it does not make sense in the context.
    ///
    /// For example, a cellbase transaction is not allowed in `send_transaction` RPC.
    PoolRejectedMalformedTransaction = -1108,
    /// (-1109): The transaction is expired from tx-pool after `expiry_hours`.
    TransactionExpired = -1109,
}
