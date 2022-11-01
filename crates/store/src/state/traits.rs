use gw_common::smt::SMT;
use gw_types::packed::LogItem;

use super::state_db::StateTracker;

pub trait SMTTree<S> {
    fn smt_tree(&self) -> SMT<S>;
}

pub trait JournalDB {
    fn snapshot(&mut self) -> usize;
    fn revert(&mut self, id: usize) -> Result<(), gw_common::error::Error>;
    fn appended_logs(&self) -> &im::Vector<LogItem>;
    fn append_log(&mut self, log: LogItem);
    fn finalise(&mut self) -> Result<(), gw_common::error::Error>;
    fn set_state_tracker(&mut self, tracker: StateTracker);
    fn state_tracker(&self) -> Option<&StateTracker>;
    fn take_state_tracker(&mut self) -> Option<StateTracker>;
}
