use anyhow::{anyhow, Result};
use gw_config::{BackendType, FeeConfig};
use gw_types::{
    packed::{
        L2Transaction, MetaContractArgs, MetaContractArgsUnion, SUDTArgs, SUDTArgsUnion,
        WithdrawalRequest,
    },
    prelude::{Entity, Unpack},
};
use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, BinaryHeap},
    convert::TryInto,
    slice::SliceIndex,
};

#[derive(PartialEq, Eq)]
pub enum FeeItem {
    Tx(L2Transaction),
    Withdrawal(WithdrawalRequest),
}

impl FeeItem {
    pub fn nonce(&self) -> u32 {
        match self {
            Self::Tx(tx) => tx.raw().nonce().unpack(),
            Self::Withdrawal(withdraw) => withdraw.raw().nonce().unpack(),
        }
    }

    fn inner_slice(&self) -> &[u8] {
        match self {
            Self::Tx(tx) => tx.as_slice(),
            Self::Withdrawal(withdraw) => withdraw.as_slice(),
        }
    }
}

impl Ord for FeeItem {
    fn cmp(&self, other: &Self) -> Ordering {
        let ord = self.nonce().cmp(&other.nonce());
        if ord == Ordering::Equal {
            return ord;
        }
        self.inner_slice().cmp(other.inner_slice())
    }
}
impl PartialOrd for FeeItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(PartialEq, Eq)]
pub struct FeeEntry {
    /// item: tx or withdrawal
    pub item: FeeItem,
    /// sender
    pub sender: u32,
    /// fee rate
    pub fee_rate: u64,
    /// estimate cycles limit
    pub cycles_limit: u64,
}

impl PartialOrd for FeeEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FeeEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // higher fee rate is priority
        let ord = self.fee_rate.cmp(&other.fee_rate);
        if ord != Ordering::Equal {
            return ord;
        }
        // lower cycles is priority
        let ord = other.cycles_limit.cmp(&self.cycles_limit);
        dbg!(ord);
        if ord != Ordering::Equal {
            return ord;
        }
        // lower nonce is priority
        other.item.cmp(&self.item)
    }
}

/// parse tx fee rate
fn parse_l2tx_fee_rate(
    fee_config: &FeeConfig,
    raw_l2tx: &gw_types::packed::RawL2Transaction,
    backend_type: BackendType,
) -> Result<u64> {
    let raw_l2tx_args = raw_l2tx.args().raw_data();
    match backend_type {
        BackendType::Meta => {
            let meta_args = MetaContractArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee = match meta_args.to_enum() {
                MetaContractArgsUnion::CreateAccount(args) => args.fee(),
            };
            let sudt_id: u32 = fee.sudt_id().unpack();
            let weight: u64 = fee_config
                .sudt_fee_weights
                .get(&sudt_id)
                .cloned()
                .unwrap_or(fee_config.default_fee_weight);
            let fee_amount: u128 = fee.amount().unpack();
            let fee_rate = fee_amount
                .checked_div(weight.into())
                .ok_or(anyhow!("Can't calculate fee"))?;
            Ok(fee_rate.try_into()?)
        }
        BackendType::Sudt => {
            let sudt_args = SUDTArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee_amount: u128 = match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTQuery(_) => {
                    // SUDTQuery fee rate is 0
                    return Ok(0);
                }
                SUDTArgsUnion::SUDTTransfer(args) => args.fee().unpack(),
            };
            let sudt_id: u32 = raw_l2tx.to_id().unpack();
            let weight: u64 = fee_config
                .sudt_fee_weights
                .get(&sudt_id)
                .cloned()
                .unwrap_or(fee_config.default_fee_weight);
            let fee_rate = fee_amount
                .checked_div(weight.into())
                .ok_or(anyhow!("can't calculate fee"))?;
            Ok(fee_rate.try_into()?)
        }
        BackendType::Polyjuice => {
            // verify the args of a polyjuice L2TX
            // https://github.com/nervosnetwork/godwoken-polyjuice/blob/aee95c0/README.md#polyjuice-arguments
            if raw_l2tx_args.len() < (8 + 8 + 16 + 16 + 4) {
                return Err(anyhow!("Invalid PolyjuiceArgs"));
            }
            // Note: Polyjuice use CKB_SUDT to pay fee by default
            let poly_args = raw_l2tx_args.as_ref();
            let gas_price = u128::from_le_bytes(poly_args[16..32].try_into()?);
            Ok(gas_price.try_into()?)
        }
        BackendType::Unknown => Err(anyhow!("Found Unknown BackendType")),
    }
}
