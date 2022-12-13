// Import from `core` instead of from `std` since we are in no-std mode
use core::result::Result;

// Import CKB syscalls and structures
// https://nervosnetwork.github.io/ckb-std/riscv64imac-unknown-none-elf/doc/ckb_std/index.html
use gw_utils::error::Error;

/// Eth account lock
/// TODO: Revisit this contract after support the interactive challenge
pub fn main() -> Result<(), Error> {
    // reject verification signature on-chain
    Err(Error::WrongSignature)
}
