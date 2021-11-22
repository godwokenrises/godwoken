/// Transaction input's since field
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Since(u64);

impl Since {
    const LOCK_TYPE_FLAG: u64 = 1 << 63;
    const METRIC_TYPE_FLAG_MASK: u64 = 0x6000_0000_0000_0000;
    const FLAGS_MASK: u64 = 0xff00_0000_0000_0000;
    const VALUE_MASK: u64 = 0x00ff_ffff_ffff_ffff;
    const REMAIN_FLAGS_BITS: u64 = 0x1f00_0000_0000_0000;
    const LOCK_BY_BLOCK_NUMBER_MASK: u64 = 0x0000_0000_0000_0000;
    const LOCK_BY_EPOCH_MASK: u64 = 0x2000_0000_0000_0000;
    const LOCK_BY_TIMESTAMP_MASK: u64 = 0x4000_0000_0000_0000;

    pub fn new(v: u64) -> Self {
        Since(v)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn is_absolute(self) -> bool {
        self.0 & Self::LOCK_TYPE_FLAG == 0
    }

    #[inline]
    pub fn is_relative(self) -> bool {
        !self.is_absolute()
    }

    pub fn flags_is_valid(self) -> bool {
        (self.0 & Self::REMAIN_FLAGS_BITS == 0)
            && ((self.0 & Self::METRIC_TYPE_FLAG_MASK) != Self::METRIC_TYPE_FLAG_MASK)
    }

    pub fn flags(self) -> u64 {
        self.0 & Self::FLAGS_MASK
    }

    pub fn extract_lock_value(self) -> Option<LockValue> {
        let value = self.0 & Self::VALUE_MASK;
        match self.0 & Self::METRIC_TYPE_FLAG_MASK {
            //0b0000_0000
            Self::LOCK_BY_BLOCK_NUMBER_MASK => Some(LockValue::BlockNumber(value)),
            //0b0010_0000
            Self::LOCK_BY_EPOCH_MASK => Some(LockValue::EpochNumberWithFraction(
                EpochNumberWithFraction::from_full_value(value),
            )),
            //0b0100_0000
            Self::LOCK_BY_TIMESTAMP_MASK => Some(LockValue::Timestamp(value * 1000)),
            _ => None,
        }
    }
}

pub enum LockValue {
    BlockNumber(u64),
    EpochNumberWithFraction(EpochNumberWithFraction),
    Timestamp(u64),
}

impl LockValue {
    pub fn block_number(&self) -> Option<u64> {
        if let Self::BlockNumber(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn epoch(&self) -> Option<EpochNumberWithFraction> {
        if let Self::EpochNumberWithFraction(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn timestamp(&self) -> Option<u64> {
        if let Self::Timestamp(v) = self {
            Some(*v)
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub struct EpochNumberWithFraction(u64);

impl EpochNumberWithFraction {
    pub const NUMBER_OFFSET: usize = 0;
    pub const NUMBER_BITS: usize = 24;
    pub const NUMBER_MAXIMUM_VALUE: u64 = (1u64 << Self::NUMBER_BITS);
    pub const NUMBER_MASK: u64 = (Self::NUMBER_MAXIMUM_VALUE - 1);
    pub const INDEX_OFFSET: usize = Self::NUMBER_BITS;
    pub const INDEX_BITS: usize = 16;
    pub const INDEX_MAXIMUM_VALUE: u64 = (1u64 << Self::INDEX_BITS);
    pub const INDEX_MASK: u64 = (Self::INDEX_MAXIMUM_VALUE - 1);
    pub const LENGTH_OFFSET: usize = Self::NUMBER_BITS + Self::INDEX_BITS;
    pub const LENGTH_BITS: usize = 16;
    pub const LENGTH_MAXIMUM_VALUE: u64 = (1u64 << Self::LENGTH_BITS);
    pub const LENGTH_MASK: u64 = (Self::LENGTH_MAXIMUM_VALUE - 1);

    pub fn new(number: u64, index: u64, length: u64) -> EpochNumberWithFraction {
        debug_assert!(number < Self::NUMBER_MAXIMUM_VALUE);
        debug_assert!(index < Self::INDEX_MAXIMUM_VALUE);
        debug_assert!(length < Self::LENGTH_MAXIMUM_VALUE);
        debug_assert!(length > 0);
        Self::new_unchecked(number, index, length)
    }

    pub const fn new_unchecked(number: u64, index: u64, length: u64) -> Self {
        EpochNumberWithFraction(
            (length << Self::LENGTH_OFFSET)
                | (index << Self::INDEX_OFFSET)
                | (number << Self::NUMBER_OFFSET),
        )
    }

    pub fn number(self) -> u64 {
        (self.0 >> Self::NUMBER_OFFSET) & Self::NUMBER_MASK
    }

    pub fn index(self) -> u64 {
        (self.0 >> Self::INDEX_OFFSET) & Self::INDEX_MASK
    }

    pub fn length(self) -> u64 {
        (self.0 >> Self::LENGTH_OFFSET) & Self::LENGTH_MASK
    }

    pub fn full_value(self) -> u64 {
        self.0
    }

    // One caveat here, is that if the user specifies a zero epoch length either
    // delibrately, or by accident, calling to_rational() after that might
    // result in a division by zero panic. To prevent that, this method would
    // automatically rewrite the value to epoch index 0 with epoch length to
    // prevent panics
    pub fn from_full_value(value: u64) -> Self {
        let epoch = Self(value);
        if epoch.length() == 0 {
            Self::new(epoch.number(), 0, 1)
        } else {
            epoch
        }
    }
}
