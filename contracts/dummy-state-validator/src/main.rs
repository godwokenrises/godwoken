//! Dummy state validator
//!
//! This script only used in the testing environment,
//! this script will be removed soon once the JS generator integrate the state-validator script
//!

#![no_std]
#![no_main]
#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(panic_info_message)]

// define modules
// mod actions;
// mod consensus;
mod context;
mod entry;
mod error;

use ckb_std::default_alloc;
pub use validator_utils::ckb_std;

ckb_std::entry!(program_entry);
default_alloc!();

/// program entry
fn program_entry() -> i8 {
    // Call main function and return error code
    match entry::main() {
        Ok(_) => 0,
        Err(err) => err as i8,
    }
}
