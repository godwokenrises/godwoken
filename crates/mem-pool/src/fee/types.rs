use anyhow::{anyhow, Result};
use gw_common::H256;
use gw_config::{BackendType, FeeConfig};
use gw_types::{
    packed::{
        L2Transaction, MetaContractArgs, MetaContractArgsUnion, SUDTArgs, SUDTArgsUnion,
        WithdrawalRequest,
    },
    prelude::{Entity, Unpack},
};
use std::{cmp::Ordering, convert::TryInto};

const FEE_RATE_WEIGHT_BASE: u64 = 1000;

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FeeItemKind {
    Tx,
    Withdrawal,
}

#[derive(PartialEq, Eq, Clone)]
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

    pub fn kind(&self) -> FeeItemKind {
        match self {
            Self::Tx(_) => FeeItemKind::Tx,
            Self::Withdrawal(_) => FeeItemKind::Withdrawal,
        }
    }

    pub fn hash(&self) -> H256 {
        match self {
            Self::Tx(tx) => tx.hash().into(),
            Self::Withdrawal(withdrawal) => withdrawal.hash().into(),
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
        if ord != Ordering::Equal {
            return ord;
        }
        // lower nonce is priority
        other.item.cmp(&self.item)
    }
}

impl FeeEntry {
    pub fn from_tx(
        tx: L2Transaction,
        fee_config: &FeeConfig,
        backend_type: BackendType,
    ) -> Result<Self> {
        let raw_l2tx = tx.raw();
        let sender = raw_l2tx.from_id().unpack();
        let fee = parse_l2tx_fee_rate(fee_config, &raw_l2tx, backend_type)?;
        let entry = FeeEntry {
            item: FeeItem::Tx(tx),
            sender,
            fee_rate: fee.fee_rate,
            cycles_limit: fee.cycles_limit,
        };
        Ok(entry)
    }

    pub fn from_withdrawal(
        withdrawal: WithdrawalRequest,
        sender: u32,
        fee_config: &FeeConfig,
    ) -> Result<Self> {
        let raw_withdrawal = withdrawal.raw();
        let fee = parse_withdraw_fee_rate(fee_config, &raw_withdrawal)?;
        let entry = FeeEntry {
            item: FeeItem::Withdrawal(withdrawal),
            sender,
            fee_rate: fee.fee_rate,
            cycles_limit: fee.cycles_limit,
        };
        Ok(entry)
    }
}

struct L2Fee {
    fee_rate: u64,
    cycles_limit: u64,
}

fn parse_withdraw_fee_rate(
    fee_config: &FeeConfig,
    raw_withdraw: &gw_types::packed::RawWithdrawalRequest,
) -> Result<L2Fee> {
    let fee = raw_withdraw.fee();
    let sudt_id: u32 = fee.sudt_id().unpack();
    let cycles_limit: u64 = fee_config.withdraw_cycles_limit;
    let fee_rate_weight = fee_config
        .sudt_fee_rate_weight
        .get(&sudt_id.into())
        .cloned()
        .unwrap_or(FEE_RATE_WEIGHT_BASE);
    let fee_amount: u128 = fee.amount().unpack();
    let fee_rate = fee_amount
        .saturating_mul(fee_rate_weight.into())
        .checked_div(cycles_limit.saturating_mul(FEE_RATE_WEIGHT_BASE).into())
        .ok_or(anyhow!("can't calculate fee"))?;
    Ok(L2Fee {
        fee_rate: fee_rate.try_into()?,
        cycles_limit,
    })
}

/// parse tx fee rate
fn parse_l2tx_fee_rate(
    fee_config: &FeeConfig,
    raw_l2tx: &gw_types::packed::RawL2Transaction,
    backend_type: BackendType,
) -> Result<L2Fee> {
    let raw_l2tx_args = raw_l2tx.args().raw_data();
    match backend_type {
        BackendType::Meta => {
            let meta_args = MetaContractArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee = match meta_args.to_enum() {
                MetaContractArgsUnion::CreateAccount(args) => args.fee(),
            };
            let sudt_id: u32 = fee.sudt_id().unpack();
            let cycles_limit: u64 = fee_config.meta_cycles_limit;
            let fee_amount: u128 = fee.amount().unpack();
            let fee_rate_weight = fee_config
                .sudt_fee_rate_weight
                .get(&sudt_id.into())
                .cloned()
                .unwrap_or(FEE_RATE_WEIGHT_BASE);

            // fee rate = fee / cycles_limit * (weight / FEE_RATE_WEIGHT_BASE)
            let fee_rate = fee_amount
                .saturating_mul(fee_rate_weight.into())
                .checked_div(cycles_limit.saturating_mul(FEE_RATE_WEIGHT_BASE).into())
                .ok_or(anyhow!("can't calculate fee"))?;

            Ok(L2Fee {
                fee_rate: fee_rate.try_into()?,
                cycles_limit,
            })
        }
        BackendType::Sudt => {
            let sudt_args = SUDTArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee_amount: u128 = match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTQuery(_) => {
                    // SUDTQuery fee rate is 0
                    0
                }
                SUDTArgsUnion::SUDTTransfer(args) => args.fee().unpack(),
            };
            let sudt_id: u32 = raw_l2tx.to_id().unpack();
            let cycles_limit: u64 = fee_config.sudt_cycles_limit;
            let fee_rate_weight = fee_config
                .sudt_fee_rate_weight
                .get(&sudt_id.into())
                .cloned()
                .unwrap_or(FEE_RATE_WEIGHT_BASE);

            // fee rate = fee / cycles_limit * (weight / FEE_RATE_WEIGHT_BASE)
            let fee_rate = fee_amount
                .saturating_mul(fee_rate_weight.into())
                .checked_div(cycles_limit.saturating_mul(FEE_RATE_WEIGHT_BASE).into())
                .ok_or(anyhow!("can't calculate fee"))?;
            Ok(L2Fee {
                fee_rate: fee_rate.try_into()?,
                cycles_limit,
            })
        }
        BackendType::Polyjuice => {
            // verify the args of a polyjuice L2TX
            // https://github.com/nervosnetwork/godwoken-polyjuice/blob/aee95c0/README.md#polyjuice-arguments
            if raw_l2tx_args.len() < (8 + 8 + 16 + 16 + 4) {
                return Err(anyhow!("Invalid PolyjuiceArgs"));
            }
            // Note: Polyjuice use CKB_SUDT to pay fee by default
            let poly_args = raw_l2tx_args.as_ref();
            let gas_limit = u64::from_le_bytes(poly_args[8..16].try_into()?);
            let gas_price = u128::from_le_bytes(poly_args[16..32].try_into()?);
            Ok(L2Fee {
                fee_rate: gas_price.try_into()?,
                cycles_limit: gas_limit,
            })
        }
        BackendType::Unknown => Err(anyhow!("Found Unknown BackendType")),
    }
}
