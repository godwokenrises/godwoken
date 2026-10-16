#![no_std]
extern crate alloc;

pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/exit.rs"));
}
pub mod codec;
pub mod verify;
pub use verify::*;
include!(concat!(env!("OUT_DIR"), "/profile_hash.rs"));

pub mod settlement;

pub mod native;
pub mod v0;

pub mod control;

pub mod proof_backend;

pub mod transport;

#[cfg(test)]
extern crate std;
#[cfg(test)]
#[path = "tests/native.rs"]
mod native_tests;
