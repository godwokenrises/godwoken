//! Approved frozen-snapshot policy decisions. No amounts, shares or prices.
//! Low-value decisions apply to otherwise unresolved residues only; proven
//! principal routes are evaluated first. These are not live valuation rules.
use anyhow::{bail, Result};
const LOW_VALUE: &[u64] = &[
    9, 10, 12, 16, 18, 19, 33, 133, 166, 168, 169, 228, 230, 231, 1792, 1811, 2070, 2076, 2315,
    2319, 2357, 3174, 3176, 3178, 3180, 3182, 3335, 3463, 3477, 4224, 4299, 4361, 5731, 5830, 5832,
    6367, 7789, 9947, 10098, 10099, 10103, 10108, 10110, 10111, 10934, 11064, 11066, 11068, 11070,
    11072, 11119, 11120, 11222, 11484, 11580, 11909, 11957, 12145, 12147, 12282, 12562, 13044,
];
pub fn residual(owner: Option<u64>, asset: u64, address: &str) -> Result<&'static str> {
    match (owner, asset) {
        (Some(121), 120) => Ok("HOLD_MANUAL_TRANSFER_CLAIM"),
        (Some(5826), 1) => Ok("EXCLUDE_UNCLAIMED_CKB_REWARD"),
        (Some(3172), 128) => Ok("EXCLUDE_IDLE_YOK_BONUS"),
        (Some(11736 | 12028), _) => Ok("RETAIN_APPROVED_LOW_VALUE_RELAYER"),
        (Some(4466 | 12228 | 12229), 1) => Ok("HOLD_PROJECT_SALE_CUSTODY"),
        (Some(4831 | 5713 | 10288 | 10317), _) => Ok("HOLD_NON_MULTISIG_PROJECT_CONTROL"),
        (Some(7667 | 9469 | 9427 | 11992 | 11353 | 12043 | 987 | 12394), _) => {
            Ok("IGNORE_APPROVED_RESEARCH_SCOPE")
        }
        (Some(id), _) if LOW_VALUE.contains(&id) => Ok("IGNORE_RESIDUAL_BELOW_100"),
        (None, _)
            if matches!(
                address,
                "0x6db12d65c3b43d3d34a285bdae31da1631f2296f"
                    | "0xb7f77f955ac4125abfc38ba422a38f0c099dafa3"
            ) =>
        {
            Ok("RETAIN_UNREGISTERED_IDENTITY")
        }
        (None, _) if address == "0x12ce6609405df7072d9e1a8e794c403d7dbec25f" => {
            Ok("DEFER_LP12697_LOW_VALUE")
        }
        _ => bail!("No approved residual disposition for {owner:?}, asset {asset}, {address}"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_residuals_fail_closed() {
        assert!(residual(Some(999999), 1, "").is_err());
        assert!(residual(None, 1, "0x1234").is_err());
        assert!(residual(Some(5826), 38, "").is_err());
    }
    #[test]
    fn approved_exception_is_asset_specific() {
        assert_eq!(
            residual(Some(121), 120, "").unwrap(),
            "HOLD_MANUAL_TRANSFER_CLAIM"
        );
        assert!(residual(Some(121), 1, "").is_err());
        assert_eq!(LOW_VALUE.len(), 62);
    }
}
