use gw_db::schema::Col;

pub mod mem_pool_smt_store;
pub mod mem_smt_store;
pub mod serde;
pub mod smt_store;

pub struct Columns {
    pub leaf_col: Col,
    pub branch_col: Col,
}
