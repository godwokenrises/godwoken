use std::convert::TryInto;

use ckb_vm::Bytes;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_types::{
    core::AllowedContractType,
    packed::{ETHAddrRegArgsReader, MetaContractArgsReader, RawL2Transaction, SUDTArgsReader},
    prelude::*,
    U256,
};

/// Types Transaction
pub enum TypedRawTransaction {
    EthAddrReg(EthAddrRegTx),
    Meta(MetaTx),
    SimpleUDT(SimpleUDTTx),
    Polyjuice(PolyjuiceTx),
}

impl TypedRawTransaction {
    pub fn from_tx(raw_tx: RawL2Transaction, type_: AllowedContractType) -> Option<Self> {
        let tx = match type_ {
            AllowedContractType::EthAddrReg => Self::EthAddrReg(EthAddrRegTx(raw_tx)),
            AllowedContractType::Meta => Self::Meta(MetaTx(raw_tx)),
            AllowedContractType::Sudt => Self::SimpleUDT(SimpleUDTTx(raw_tx)),
            AllowedContractType::Polyjuice => Self::Polyjuice(PolyjuiceTx(raw_tx)),
            AllowedContractType::Unknown => return None,
        };
        Some(tx)
    }

    /// Got expect cost of the tx, (transfer value + maximum fee).
    /// returns none if tx has no cost, it may happend when we call readonly interface of some Godwoken builtin contracts.
    pub fn cost(&self) -> Option<U256> {
        match self {
            Self::EthAddrReg(tx) => tx.cost(),
            Self::Meta(tx) => tx.cost(),
            Self::SimpleUDT(tx) => tx.cost(),
            Self::Polyjuice(tx) => tx.cost(),
        }
    }
}

pub struct EthAddrRegTx(RawL2Transaction);

impl EthAddrRegTx {
    pub fn consumed(&self) -> Option<U256> {
        use gw_types::packed::ETHAddrRegArgsUnionReader::*;

        let args: Bytes = self.0.args().unpack();
        let args = ETHAddrRegArgsReader::from_slice(&args).ok()?;

        match args.to_enum() {
            EthToGw(_) | GwToEth(_) => None,
            SetMapping(args) => Some(args.fee().amount().unpack().into()),
            BatchSetMapping(args) => Some(args.fee().amount().unpack().into()),
        }
    }

    pub fn cost(&self) -> Option<U256> {
        self.consumed()
    }
}

pub struct MetaTx(RawL2Transaction);

impl MetaTx {
    pub fn consumed(&self) -> Option<U256> {
        use gw_types::packed::MetaContractArgsUnionReader::*;

        let args: Bytes = self.0.args().unpack();
        let args = MetaContractArgsReader::from_slice(&args).ok()?;

        match args.to_enum() {
            CreateAccount(args) => Some(args.fee().amount().unpack().into()),
        }
    }

    pub fn cost(&self) -> Option<U256> {
        self.consumed()
    }
}

pub struct SimpleUDTTx(RawL2Transaction);

impl SimpleUDTTx {
    pub fn consumed(&self) -> Option<U256> {
        use gw_types::packed::SUDTArgsUnionReader::*;

        let args: Bytes = self.0.args().unpack();
        let args = SUDTArgsReader::from_slice(&args).ok()?;

        match args.to_enum() {
            SUDTQuery(_) => None,
            SUDTTransfer(args) => {
                let fee = args.fee().amount().unpack();
                Some(fee.into())
            }
        }
    }

    pub fn cost(&self) -> Option<U256> {
        use gw_types::packed::SUDTArgsUnionReader::*;

        let args: Bytes = self.0.args().unpack();
        let args = SUDTArgsReader::from_slice(&args).ok()?;

        match args.to_enum() {
            SUDTQuery(_) => None,
            SUDTTransfer(args) => {
                let fee = args.fee().amount().unpack();
                let to_id: u32 = self.0.to_id().unpack();
                if to_id == CKB_SUDT_ACCOUNT_ID {
                    // CKB transfer cost: transfer CKB value + fee
                    let value = args.amount().unpack();
                    value.checked_add(fee.into())
                } else {
                    // Simple UDT transfer cost: fee
                    Some(fee.into())
                }
            }
        }
    }
}

pub struct PolyjuiceTxArgs {
    pub value: u128,
    pub gas_price: u128,
    pub gas_limit: u64,
}

pub struct PolyjuiceTx(RawL2Transaction);
impl PolyjuiceTx {
    pub fn extract_tx_args(&self) -> Option<PolyjuiceTxArgs> {
        let args: Bytes = self.0.args().unpack();
        if args.len() < 52 {
            log::error!(
                "[gw-generator] parse PolyjuiceTx error, wrong args.len expected: >= 52, actual: {}",
                args.len()
            );
            return None;
        }
        if args[0..7] != b"\xFF\xFF\xFFPOLY"[..] {
            log::error!("[gw-generator] parse PolyjuiceTx error, invalid args",);
            return None;
        }

        // parse gas price, gas limit, value
        let gas_price = {
            let mut data = [0u8; 16];
            data.copy_from_slice(&args[16..32]);
            u128::from_le_bytes(data)
        };
        let gas_limit = {
            let mut data = [0u8; 8];
            data.copy_from_slice(&args[8..16]);
            u64::from_le_bytes(data)
        };

        let value = {
            let mut data = [0u8; 16];
            data.copy_from_slice(&args[32..48]);
            u128::from_le_bytes(data)
        };
        Some(PolyjuiceTxArgs {
            value,
            gas_price,
            gas_limit,
        })
    }

    /// Total cost of a tx, sender's balance must sufficient to pay Cost(value + gas_price * gas_limit)
    pub fn cost(&self) -> Option<U256> {
        match self.extract_tx_args() {
            Some(PolyjuiceTxArgs {
                value,
                gas_price,
                gas_limit,
            }) => {
                let cost = value.checked_add(gas_price.checked_mul(gas_limit.into())?)?;
                cost.try_into().ok()
            }
            None => None,
        }
    }
}
