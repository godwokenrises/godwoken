pub use molecule::prelude::*;

pub trait Unpack<T> {
    fn unpack(&self) -> T;
}

pub trait Pack<T: Entity> {
    fn pack(&self) -> T;
}

pub trait PackVec<T: Entity, I: Entity>: IntoIterator<Item = I> {
    fn pack(self) -> T;
}

// Make migrating to using ckb-types easier.
pub trait CalcHash {
    fn hash(&self) -> crate::h256::H256;
}

impl CalcHash for crate::packed::Script {
    fn hash(&self) -> crate::h256::H256 {
        gw_hash::blake2b::hash(self.as_slice())
    }
}

impl CalcHash for crate::packed::Transaction {
    fn hash(&self) -> crate::h256::H256 {
        gw_hash::blake2b::hash(self.as_reader().raw().as_slice())
    }
}

#[cfg(feature = "std")]
pub trait OccupiedCapacityBytes {
    fn occupied_capacity_bytes(&self, len: usize) -> Result<u64, ckb_types::core::CapacityError>;
}

#[cfg(feature = "std")]
impl OccupiedCapacityBytes for crate::packed::CellOutput {
    fn occupied_capacity_bytes(&self, len: usize) -> Result<u64, ckb_types::core::CapacityError> {
        Ok(self
            .occupied_capacity(ckb_types::core::Capacity::bytes(len)?)?
            .as_u64())
    }
}

/// An alias of `from_slice(..)` to mark where we are really have confidence to do unwrap on the result of `from_slice(..)`.
pub trait FromSliceShouldBeOk<'r>: Reader<'r> {
    /// Unwraps the result of `from_slice(..)` with confidence and we assume that it's impossible to fail.
    fn from_slice_should_be_ok(slice: &'r [u8]) -> Self;
}

impl<'r, R> FromSliceShouldBeOk<'r> for R
where
    R: Reader<'r>,
{
    fn from_slice_should_be_ok(slice: &'r [u8]) -> Self {
        match Self::from_slice(slice) {
            Ok(ret) => ret,
            Err(_err) => panic!("invalid molecule structure"),
        }
    }
}
