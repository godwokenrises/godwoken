pub mod abort_on_drop;
pub mod compression;
pub mod exponential_backoff;
pub mod export_block;
pub mod fee;
pub mod genesis_info;
pub mod local_cells;
pub mod polyjuice_parser;
mod query_rollup_cell;
pub mod script_log;
pub mod since;
pub mod transaction_skeleton;
pub mod wallet;
pub mod withdrawal;

pub use query_rollup_cell::query_rollup_cell;

pub fn block_in_place_if_not_testing<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    if cfg!(test) {
        tokio::task::block_in_place(f)
    } else {
        f()
    }
}
