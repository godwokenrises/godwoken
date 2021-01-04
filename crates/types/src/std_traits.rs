use crate::{packed, prelude::*};

macro_rules! impl_std_eq {
    ($struct:ident) => {
        impl PartialEq for packed::$struct {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.as_slice() == other.as_slice()
            }
        }
        impl Eq for packed::$struct {}
    };
}

impl_std_eq!(Script);
impl_std_eq!(CancelChallenge);
impl_std_eq!(TxReceipt);
impl_std_eq!(StartChallenge);
impl_std_eq!(StartChallengeWitness);
