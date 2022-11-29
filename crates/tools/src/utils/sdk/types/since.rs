use ckb_types::core::EpochNumberWithFraction;

use crate::utils::sdk::constants::{
    LOCK_TYPE_FLAG, METRIC_TYPE_FLAG_MASK, REMAIN_FLAGS_BITS, VALUE_MASK,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinceType {
    BlockNumber,
    EpochNumberWithFraction,
    Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Since(u64);

impl Since {
    pub fn new(ty: SinceType, value: u64, is_relative: bool) -> Since {
        let value = match ty {
            SinceType::BlockNumber => value,
            SinceType::EpochNumberWithFraction => 0x2000_0000_0000_0000 | value,
            SinceType::Timestamp => 0x4000_0000_0000_0000 | value,
        };
        if is_relative {
            Since(LOCK_TYPE_FLAG | value)
        } else {
            Since(value)
        }
    }

    pub fn new_absolute_epoch(epoch_number: u64) -> Since {
        let epoch = EpochNumberWithFraction::new(epoch_number, 0, 1);
        Self::new(
            SinceType::EpochNumberWithFraction,
            epoch.full_value(),
            false,
        )
    }

    pub fn from_raw_value(value: u64) -> Since {
        Since(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    pub fn is_absolute(self) -> bool {
        self.0 & LOCK_TYPE_FLAG == 0
    }

    pub fn is_relative(self) -> bool {
        !self.is_absolute()
    }

    pub fn flags_is_valid(self) -> bool {
        (self.0 & REMAIN_FLAGS_BITS == 0)
            && ((self.0 & METRIC_TYPE_FLAG_MASK) != METRIC_TYPE_FLAG_MASK)
    }

    pub fn extract_metric(self) -> Option<(SinceType, u64)> {
        let value = self.0 & VALUE_MASK;
        let ty_opt = match self.0 & METRIC_TYPE_FLAG_MASK {
            //0b0000_0000
            0x0000_0000_0000_0000 => Some(SinceType::BlockNumber),
            //0b0010_0000
            0x2000_0000_0000_0000 => Some(SinceType::EpochNumberWithFraction),
            //0b0100_0000
            0x4000_0000_0000_0000 => Some(SinceType::Timestamp),
            _ => None,
        };
        ty_opt.map(|ty| (ty, value))
    }
}
