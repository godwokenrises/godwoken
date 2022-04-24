use gw_types::U256;

// NOTE: u64::MAX is about 10^18, U256::MAX is about 10^77. Multiple 10 pow
// `CKB_DECIMAL_POW_EXP` should not overflow.
pub const CKB_DECIMAL_POW_EXP: u32 = 10;
pub const CKB_DECIMAL_POWER_TEN: u64 = 10u64.pow(CKB_DECIMAL_POW_EXP);

pub fn to_18(amount: u64) -> U256 {
    U256::from(amount) * CKB_DECIMAL_POWER_TEN
}

pub fn from_18(amount: U256) -> Option<u64> {
    let truncated = amount / CKB_DECIMAL_POWER_TEN;
    if truncated.bits() > u64::BITS as usize {
        None
    } else {
        Some(truncated.as_u64())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assertions_on_constants)]
    use super::{CKB_DECIMAL_POWER_TEN, CKB_DECIMAL_POW_EXP};

    #[test]
    fn test_ckb_decimal_exp() {
        assert!(CKB_DECIMAL_POW_EXP < 18);
        assert_eq!(
            10u64.checked_pow(CKB_DECIMAL_POW_EXP),
            Some(CKB_DECIMAL_POWER_TEN)
        );
    }
}
