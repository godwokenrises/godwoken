The following metrics are exposed on the `/metrics` path on the http server:

## Full node

```
# HELP gw_chain_transactions Number of packaged L2 transactions.
# TYPE gw_chain_transactions counter
gw_chain_transactions_total 60
# HELP gw_chain_deposits Number of packaged deposits.
# TYPE gw_chain_deposits counter
gw_chain_deposits_total 0
# HELP gw_chain_withdrawals Number of packaged withdrawals.
# TYPE gw_chain_withdrawals counter
gw_chain_withdrawals_total 0
# HELP gw_chain_block_height Number of the highest known block.
# TYPE gw_chain_block_height gauge
gw_chain_block_height 30761
# HELP gw_rpc_in_queue_requests Number of in queue requests.
# TYPE gw_rpc_in_queue_requests gauge
gw_rpc_in_queue_requests 0
# HELP gw_rpc_execute_transactions Number of execute_transaction requests.
# TYPE gw_rpc_execute_transactions counter
# HELP gw_psc_local_blocks Number of local blocks.
# TYPE gw_psc_local_blocks gauge
gw_psc_local_blocks 0
# HELP gw_psc_submitted_blocks Number of submitted blocks.
# TYPE gw_psc_submitted_blocks gauge
gw_psc_submitted_blocks 5
# HELP gw_psc_resend Number of times resending submission transactions.
# TYPE gw_psc_resend counter
gw_psc_resend_total 0
# HELP gw_psc_witness_size_bytes Block submission txs witness size.
# TYPE gw_psc_witness_size_bytes counter
# UNIT gw_psc_witness_size_bytes bytes
gw_psc_witness_size_bytes_total 370192
# HELP gw_psc_tx_size_bytes Block submission txs size.
# TYPE gw_psc_tx_size_bytes counter
# UNIT gw_psc_tx_size_bytes bytes
gw_psc_tx_size_bytes_total 574481
# EOF
```

## Read-only node

```
# HELP gw_sync_buffer_len Number of messages in the block sync receive buffer.
# TYPE gw_sync_buffer_len gauge
gw_sync_buffer_len 0
# HELP gw_chain_transactions Number of packaged L2 transactions.
# TYPE gw_chain_transactions counter
gw_chain_transactions_total 60
# HELP gw_chain_deposits Number of packaged deposits.
# TYPE gw_chain_deposits counter
gw_chain_deposits_total 0
# HELP gw_chain_withdrawals Number of packaged withdrawals.
# TYPE gw_chain_withdrawals counter
gw_chain_withdrawals_total 0
# HELP gw_chain_block_height Number of the highest known block.
# TYPE gw_chain_block_height gauge
gw_chain_block_height 30765
# HELP gw_rpc_execute_transactions Number of execute_transaction requests.
# TYPE gw_rpc_execute_transactions counter
gw_rpc_execute_transactions_total{exit_code="0"} 202
gw_rpc_execute_transactions_total{exit_code="2"} 13
# EOF
```
