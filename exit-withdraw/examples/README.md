# Developer diagnostics

Refund generation and operator workflows use `gw-withdraw`; see the [operator runbook](../docs/ops-refund-runbook.md). Historical table-driven calculators and one-off contract research programs have been removed. They are not alternative refund generators.

Only these developer helpers remain:

- `db_getter_check`: independently executes deployed V1 getters against a read-only DB, optionally checking freshly extracted state facts. It does not create a refund table.
- `prepare_v0_rehearsal`: prepares a synthetic V0 VM profile and snapshot fixture. Its output is test data, never a production deployment profile.

Run a helper with `cargo +1.94.0 run --locked --example NAME -- ARGUMENTS`. Each helper reports its required arguments if invoked without them. Original research programs remain available in Git history.
