use std::cmp::Ordering;

use anyhow::{anyhow, ensure, Context, Result};
use gw_config::{BackendType, FeeConfig, GaslessTxSupportConfig};
use gw_types::{
    h256::*,
    packed::{
        ETHAddrRegArgs, ETHAddrRegArgsUnion, L2Transaction, MetaContractArgs,
        MetaContractArgsUnion, SUDTArgs, SUDTArgsUnion, WithdrawalRequestExtra,
    },
    prelude::*,
};
use gw_utils::{
    gasless::{gasless_tx_fee, is_gasless_tx},
    polyjuice_parser::PolyjuiceParser,
};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum FeeItemKind {
    Tx,
    PendingCreateSenderTx,
    Withdrawal,
}

#[derive(PartialEq, Eq, Clone)]
pub enum FeeItem {
    Tx(L2Transaction),
    Withdrawal(WithdrawalRequestExtra),
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
            Self::Tx(tx) if 0 == Unpack::<u32>::unpack(&tx.raw().from_id()) => {
                FeeItemKind::PendingCreateSenderTx
            }
            Self::Tx(_) => FeeItemKind::Tx,
            Self::Withdrawal(_) => FeeItemKind::Withdrawal,
        }
    }

    pub fn hash(&self) -> H256 {
        match self {
            Self::Tx(tx) if self.kind() == FeeItemKind::PendingCreateSenderTx => {
                let sig: gw_types::bytes::Bytes = tx.signature().unpack();
                gw_common::blake2b::hash(&sig)
            }
            Self::Tx(tx) => tx.hash(),
            Self::Withdrawal(withdrawal) => withdrawal.hash(),
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

#[derive(PartialEq, Eq, Clone, Copy, Debug, Hash)]
pub enum FeeItemSender {
    AccountId(u32),
    PendingCreate(H256), // hash
}

#[derive(PartialEq, Eq, Clone)]
pub struct FeeEntry {
    /// item: tx or withdrawal
    pub item: FeeItem,
    /// Order in queue: queue.len() when insertion
    pub order: usize,
    /// sender
    pub sender: FeeItemSender,
    /// fee
    pub fee: u128,
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
        // A / B > C / D => A * D > C * B
        // higher fee rate is priority
        let ord = self
            .fee
            .saturating_mul(other.cycles_limit.into())
            .cmp(&other.fee.saturating_mul(self.cycles_limit.into()));
        if ord != Ordering::Equal {
            return ord;
        }
        // lower order is priority
        let ord = other.order.cmp(&self.order);
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
        gasless_tx_support_config: Option<&GaslessTxSupportConfig>,
        fee_config: &FeeConfig,
        backend_type: BackendType,
        order: usize,
    ) -> Result<Self> {
        let raw_l2tx = tx.raw();
        let fee = parse_l2tx_fee_rate(
            gasless_tx_support_config,
            fee_config,
            &raw_l2tx,
            backend_type,
        )?;
        let item = FeeItem::Tx(tx);

        let from_id: u32 = raw_l2tx.from_id().unpack();
        let sender = if 0 == from_id {
            FeeItemSender::PendingCreate(item.hash())
        } else {
            FeeItemSender::AccountId(from_id)
        };

        let entry = FeeEntry {
            item,
            sender,
            fee: fee.fee,
            cycles_limit: fee.cycles_limit,
            order,
        };

        Ok(entry)
    }

    pub fn from_withdrawal(
        withdrawal: WithdrawalRequestExtra,
        sender: u32,
        fee_config: &FeeConfig,
        order: usize,
    ) -> Result<Self> {
        let raw_withdrawal = withdrawal.raw();
        let fee = parse_withdraw_fee_rate(fee_config, &raw_withdrawal)?;
        let entry = FeeEntry {
            item: FeeItem::Withdrawal(withdrawal),
            sender: FeeItemSender::AccountId(sender),
            fee: fee.fee,
            cycles_limit: fee.cycles_limit,
            order,
        };
        Ok(entry)
    }
}

struct L2Fee {
    fee: u128,
    cycles_limit: u64,
}

fn parse_withdraw_fee_rate(
    fee_config: &FeeConfig,
    raw_withdraw: &gw_types::packed::RawWithdrawalRequest,
) -> Result<L2Fee> {
    let fee = raw_withdraw.fee();
    let cycles_limit: u64 = fee_config.withdraw_cycles_limit;
    Ok(L2Fee {
        fee: fee.unpack(),
        cycles_limit,
    })
}

/// parse tx fee rate
fn parse_l2tx_fee_rate(
    gasless_tx_support_config: Option<&GaslessTxSupportConfig>,
    fee_config: &FeeConfig,
    raw_l2tx: &gw_types::packed::RawL2Transaction,
    backend_type: BackendType,
) -> Result<L2Fee> {
    let raw_l2tx_args = raw_l2tx.args().raw_data();
    match backend_type {
        BackendType::Meta => {
            let meta_args = MetaContractArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee = match meta_args.to_enum() {
                MetaContractArgsUnion::CreateAccount(args) => args.fee().amount().unpack(),
                MetaContractArgsUnion::BatchCreateEthAccounts(args) => args.fee().amount().unpack(),
            };
            let cycles_limit: u64 = fee_config.meta_cycles_limit;

            Ok(L2Fee { fee, cycles_limit })
        }
        BackendType::EthAddrReg => {
            let eth_addr_reg_args = ETHAddrRegArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee = match eth_addr_reg_args.to_enum() {
                ETHAddrRegArgsUnion::EthToGw(_) | ETHAddrRegArgsUnion::GwToEth(_) => 0,
                ETHAddrRegArgsUnion::SetMapping(args) => args.fee().amount().unpack(),
                ETHAddrRegArgsUnion::BatchSetMapping(args) => args.fee().amount().unpack(),
            };
            Ok(L2Fee {
                fee,
                cycles_limit: fee_config.eth_addr_reg_cycles_limit,
            })
        }
        BackendType::Sudt => {
            let sudt_args = SUDTArgs::from_slice(raw_l2tx_args.as_ref())?;
            let fee = match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTQuery(_) => {
                    // SUDTQuery fee rate is 0
                    0
                }
                SUDTArgsUnion::SUDTTransfer(args) => args.fee().amount().unpack(),
            };
            let cycles_limit: u64 = fee_config.sudt_cycles_limit;

            Ok(L2Fee { fee, cycles_limit })
        }
        BackendType::Polyjuice => {
            let poly_args =
                PolyjuiceParser::from_raw_l2_tx(raw_l2tx).context("parse polyjuice args")?;
            // Note: Polyjuice use CKB_SUDT to pay fee by default
            let (gas_limit, gas_price) = if poly_args.gas_price() > 0 {
                (poly_args.gas(), poly_args.gas_price())
            } else {
                // Check possible gasless tx.
                if is_gasless_tx(gasless_tx_support_config, &poly_args) {
                    let data = poly_args.data();
                    let fee = gasless_tx_fee(data).context("get gasless tx fee from payload")?;
                    ensure!(poly_args.gas() == fee.gas_limit);
                    (fee.gas_limit, fee.gas_price)
                } else {
                    (poly_args.gas(), poly_args.gas_price())
                }
            };
            Ok(L2Fee {
                fee: gas_price.saturating_mul(gas_limit.into()),
                cycles_limit: gas_limit,
            })
        }
        BackendType::Unknown => Err(anyhow!("Found Unknown BackendType")),
    }
}
