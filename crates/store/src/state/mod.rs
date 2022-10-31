//! State
//!
//! - state_db: the wrapper of all state layers
//! - overlay: memory overlay layer
//! - history: history block state layer

pub mod history;
pub mod overlay;
pub mod state_db;
pub mod traits;

// alias types
pub type MemStateDB = state_db::StateDB<overlay::mem_state::MemStateTree>;
pub type BlockStateDB<S> = state_db::StateDB<history::history_state::HistoryState<S>>;
