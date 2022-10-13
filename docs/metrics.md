The following metrics are exposed on the `/metrics` path on the http server:

## Full node

```
# HELP gw_transactions number of packaged L2 transactions.
# TYPE gw_transactions counter
gw_transactions_total 28
# HELP gw_deposits number of packaged deposits.
# TYPE gw_deposits counter
gw_deposits_total 0
# HELP gw_withdrawals number of packaged withdrawals.
# TYPE gw_withdrawals counter
gw_withdrawals_total 0
# HELP gw_block_height layer 2 block height.
# TYPE gw_block_height gauge
gw_block_height 25146
# HELP gw_in_queue_requests number of in queue requests.
# TYPE gw_in_queue_requests gauge
gw_in_queue_requests 0
# HELP gw_execute_transactions number of execute_transaction requests.
# TYPE gw_execute_transactions counter
# HELP gw_psc_resend number of times resending submission transactions.
# TYPE gw_psc_resend counter
gw_psc_resend_total 0
# HELP gw_psc_local_blocks number of local blocks.
# TYPE gw_psc_local_blocks gauge
gw_psc_local_blocks 0
# HELP gw_psc_submitted_blocks number of submitted (but not yet confirmed) blocks.
# TYPE gw_psc_submitted_blocks gauge
gw_psc_submitted_blocks 6
# EOF
```

## Read-only node

```
# HELP gw_transactions number of packaged L2 transactions.
# TYPE gw_transactions counter
gw_transactions_total 20
# HELP gw_deposits number of packaged deposits.
# TYPE gw_deposits counter
gw_deposits_total 0
# HELP gw_withdrawals number of packaged withdrawals.
# TYPE gw_withdrawals counter
gw_withdrawals_total 0
# HELP gw_block_height layer 2 block height.
# TYPE gw_block_height gauge
gw_block_height 25141
# HELP gw_execute_transactions number of execute_transaction requests.
# TYPE gw_execute_transactions counter
gw_execute_transactions_total{exit_code="2"} 6
gw_execute_transactions_total{exit_code="0"} 35
# EOF
```
