use gw_types::U256;

// NOTE: u64::MAX is about 10^18, U256::MAX is about 10^77. Multiple 10 pow
// `CKB_DECIMAL_POW_EXP` should not overflow.
pub const CKB_DECIMAL_POW_EXP: u32 = 10;
pub const CKB_DECIMAL_POWER_TEN: u64 = 10u64.pow(CKB_DECIMAL_POW_EXP);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct CKBCapacity(U256);

impl CKBCapacity {
    pub fn from_layer1(amount: u64) -> Self {
        CKBCapacity(U256::from(amount) * CKB_DECIMAL_POWER_TEN)
    }

    pub fn from_layer2(amount: U256) -> Self {
        CKBCapacity(amount)
    }

    pub fn to_layer1(&self) -> Option<u64> {
        let truncated = self.0 / CKB_DECIMAL_POWER_TEN;
        if truncated.bits() > u64::BITS as usize {
            None
        } else {
            Some(truncated.as_u64())
        }
    }

    pub fn to_layer2(&self) -> U256 {
        self.0
    }
}
