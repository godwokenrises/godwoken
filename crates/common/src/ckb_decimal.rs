use gw_types::U256;

// NOTE: u64::MAX is about 10^18, U256::MAX is about 10^77. Multiple 10 pow
// `CKB_DECIMAL_POW_EXP` should not overflow.
pub const CKB_DECIMAL_POW_EXP: u32 = 10;
pub const CKB_DECIMAL_POWER_TEN: u64 = 10u64.pow(CKB_DECIMAL_POW_EXP);

pub fn to_18(amount: u64) -> U256 {
    U256::from(amount) * CKB_DECIMAL_POWER_TEN
}

pub fn from_18(amount: U256) -> U256 {
    amount / CKB_DECIMAL_POWER_TEN
}
