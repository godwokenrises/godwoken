//! This file is copied from Godwoken `crates/types/src/core.rs`.

/// Timepoint encodes in the below layout into u64 in order to support representing two kinds of
/// time points, block number and timestamp.
///   - the highest 1 bit represent whether the time point is block-number-based or timestamp-based
///   - the rest 63 bits represent the value of time point
pub enum Timepoint {
    BlockNumber(u64),
    Timestamp(u64),
}

impl Timepoint {
    const MASK: u64 = 1 << 63;

    pub const fn from_block_number(block_number: u64) -> Self {
        Timepoint::BlockNumber(block_number)
    }

    pub const fn from_timestamp(timestamp: u64) -> Self {
        Timepoint::Timestamp(timestamp)
    }

    pub const fn from_full_value(full_value: u64) -> Self {
        let is_block_number = (Self::MASK & full_value) == 0;
        if is_block_number {
            Timepoint::BlockNumber(full_value)
        } else {
            Timepoint::Timestamp(Self::MASK ^ full_value)
        }
    }

    pub const fn full_value(&self) -> u64 {
        match self {
            Timepoint::BlockNumber(block_number) => *block_number,
            Timepoint::Timestamp(timestamp) => Self::MASK | *timestamp,
        }
    }
}
