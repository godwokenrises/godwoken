use core::cmp::Ordering;

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

macro_rules! impl_std_ord {
    ($struct:ident) => {
        impl PartialOrd for packed::$struct {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for packed::$struct {
            #[inline]
            fn cmp(&self, other: &Self) -> Ordering {
                self.as_slice().cmp(other.as_slice())
            }
        }
    };
}

impl_std_eq!(Byte32);
impl_std_ord!(Byte32);
impl_std_eq!(Script);
impl_std_eq!(TxReceipt);
impl_std_eq!(ChallengeLockArgs);
impl_std_eq!(ChallengeWitness);
impl_std_eq!(ChallengeTarget);
impl_std_eq!(HeaderInfo);
impl_std_eq!(Transaction);
impl_std_eq!(DepositionRequest);
impl_std_eq!(DepositionLockArgs);
impl_std_eq!(GlobalState);
impl_std_eq!(StakeLockArgs);
impl_std_eq!(VerifyTransactionWitness);
