pub use molecule::prelude::{Builder, Entity, Reader};

pub trait Unpack<T> {
    fn unpack(&self) -> T;
}

pub trait Pack<T: Entity> {
    fn pack(&self) -> T;
}

pub trait PackVec<T: Entity, I: Entity>: IntoIterator<Item = I> {
    fn pack(self) -> T;
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
            Err(err) => panic!(err),
        }
    }
}
