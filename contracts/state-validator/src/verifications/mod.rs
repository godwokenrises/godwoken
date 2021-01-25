use gw_types::{core::Status, packed::GlobalState};

use crate::error::Error;

pub mod challenge;
pub mod submit_block;

pub fn check_status(global_state: &GlobalState, status: Status) -> Result<(), Error> {
    let expected_status: u8 = status.into();
    let status: u8 = global_state.status().into();
    if status != expected_status {
        return Err(Error::InvalidStatus);
    }
    Ok(())
}
