use std::convert::TryInto;

use ckb_vm::Bytes;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
use gw_types::{
    core::AllowedContractType,
    packed::{ETHAddrRegArgsReader, MetaContractArgsReader, RawL2Transaction, SUDTArgsReader},
    prelude::*,
    U256,
};
use gw_utils::polyjuice_parser::PolyjuiceParser;

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

pub struct PolyjuiceTx(RawL2Transaction);
impl PolyjuiceTx {
    pub fn new(raw_tx: RawL2Transaction) -> Self {
        Self(raw_tx)
    }
    pub fn parser(&self) -> Option<PolyjuiceParser> {
        PolyjuiceParser::from_raw_l2_tx(&self.0)
    }

    /// Total cost of a tx, sender's balance must sufficient to pay Cost(value + gas_price * gas_limit)
    pub fn cost(&self) -> Option<U256> {
        match self.parser() {
            Some(parser) => {
                let cost = parser
                    .value()
                    .checked_add(parser.gas_price().checked_mul(parser.gas().into())?)?;
                cost.try_into().ok()
            }
            None => None,
        }
    }

    /// Intrinsic gas
    pub fn intrinsic_gas(&self) -> Option<u64> {
        // Minimal gas of a normal transaction
        const MIN_TX_GAS: u64 = 21000;
        // Minimal gas of a transaction that creates a contract
        const MIN_CONTRACT_CREATION_TX_GAS: u64 = 53000;
        // Gas per byte of non zero data attached to a transaction
        const DATA_NONE_ZERO_GAS: u64 = 16;
        // Gas per byte of data attached to a transaction
        const DATA_ZERO_GAS: u64 = 4;

        let p = self.parser()?;

        // Set the starting gas for the raw transaction
        let mut gas = if p.is_create() {
            MIN_CONTRACT_CREATION_TX_GAS
        } else {
            MIN_TX_GAS
        };
        if p.data_size() > 0 {
            let mut non_zeros = 0u64;
            for &b in p.data() {
                if b != 0 {
                    non_zeros += 1;
                }
            }
            // nonzero bytes gas
            gas = gas.checked_add(non_zeros.checked_mul(DATA_NONE_ZERO_GAS)?)?;
            let zeros = p.data_size() as u64 - non_zeros;
            // zero bytes gas
            gas = gas.checked_add(zeros.checked_mul(DATA_ZERO_GAS)?)?;
        }
        Some(gas)
    }
}
