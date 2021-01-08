#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

mod conversion;
pub mod core;
mod extension;
mod generated;
pub mod prelude;
mod std_traits;

pub use generated::packed;
pub use molecule::bytes;

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
        use std::borrow;
        use std::str;
        use std::string;
    } else {
        use alloc::vec;
        use alloc::borrow;
        use alloc::str;
        use alloc::string;
    }
}
