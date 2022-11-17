#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

mod conversion;
pub mod core;
mod extension;
mod generated;
pub mod prelude;
pub mod registry_address;
mod std_traits;

pub use generated::packed;
pub use molecule::bytes;
pub use primitive_types::U256;

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        use std::vec;
        use std::borrow;
        use std::str;
        use std::string;

        pub mod offchain;
        mod signature_message;
    } else {
        use alloc::vec;
        use alloc::borrow;
        use alloc::str;
        use alloc::string;
    }
}

#[macro_export]
macro_rules! from_box_should_be_ok {
    ($r:ty, $b:ident) => {{
        <$r>::from_slice_should_be_ok(&$b);
        <$r as gw_types::prelude::Reader>::Entity::new_unchecked($b.into())
    }};
}
