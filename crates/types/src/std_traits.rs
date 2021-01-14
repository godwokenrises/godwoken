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
impl_std_eq!(HeaderInfo);
impl_std_eq!(Transaction);
impl_std_eq!(DepositionRequest);
impl_std_eq!(DepositionLockArgs);
impl_std_eq!(GlobalState);
