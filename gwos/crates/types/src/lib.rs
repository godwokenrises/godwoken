#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod conversion;
pub mod core;
mod extension;
mod finality;
mod generated;
pub mod h256;
pub mod prelude;
pub mod registry_address;
mod std_traits;

pub use generated::packed;
pub use molecule::bytes;
pub use primitive_types::U256;

#[cfg(feature = "std")]
pub mod offchain;
#[cfg(feature = "std")]
mod signature_message;

#[macro_export]
macro_rules! from_box_should_be_ok {
    ($r:ty, $b:ident) => {{
        <$r>::from_slice_should_be_ok(&$b);
        <$r as gw_types::prelude::Reader>::Entity::new_unchecked($b.into())
    }};
}
