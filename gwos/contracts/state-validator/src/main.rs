//! State validator
//!
//! The main script that verifies the Rollup on-chain logic
//!

#![no_std]
#![no_main]
#![feature(lang_items)]
#![feature(alloc_error_handler)]
#![feature(panic_info_message)]
#![feature(asm_sym)]

// define modules
mod entry;
mod types;
mod verifications;

use ckb_std::default_alloc;
use core::arch::asm;
pub use gw_utils::ckb_std;

ckb_std::entry!(main);
default_alloc!();

/// program entry
#[no_mangle]
fn main() -> i8 {
    // Call main function and return error code
    match entry::main() {
        Ok(_) => 0,
        Err(err) => err as i8,
    }
}
