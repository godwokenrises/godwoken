use crate::syscalls::RunResult;
use gw_common::state::{Error, State};

pub trait StateExt {
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error>;
}

impl<S: State> StateExt for S {
    fn apply_run_result(&mut self, run_result: &RunResult) -> Result<(), Error> {
        for (k, v) in &run_result.write_values {
            self.update_raw((*k).into(), (*v).into())?;
        }
        Ok(())
    }
}
