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
impl_std_eq!(ChallengeLockArgs);
impl_std_eq!(ChallengeWitness);
impl_std_eq!(ChallengeTarget);
impl_std_eq!(Transaction);
impl_std_eq!(DepositRequest);
impl_std_eq!(DepositLockArgs);
impl_std_eq!(GlobalState);
impl_std_eq!(RollupConfig);
impl_std_eq!(StakeLockArgs);
impl_std_eq!(L2Transaction);
impl_std_eq!(WithdrawalRequest);
impl_std_eq!(VerifyTransactionWitness);
impl_std_eq!(AccountMerkleState);

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        macro_rules! impl_std_hash {
            ($struct:ident) => {
                impl std::hash::Hash for packed::$struct {
                    #[inline]
                    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                        self.as_slice().hash(state);
                    }
                }
            };
        }
        impl_std_hash!(L2Transaction);
        impl_std_hash!(WithdrawalRequest);
        impl_std_hash!(CellDep);
        impl_std_eq!(CellDep);
        impl_std_hash!(OutPoint);
        impl_std_eq!(OutPoint);
        impl_std_eq!(WitnessArgs);
        impl_std_eq!(TxReceipt);
        impl_std_eq!(L2BlockCommittedInfo);
    }
}
