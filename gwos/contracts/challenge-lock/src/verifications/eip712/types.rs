use core::convert::{TryFrom, TryInto};

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use gw_utils::{
    ckb_std::debug,
    error::Error,
    gw_types::{core::ScriptHashType, packed::RawWithdrawalRequest, prelude::Unpack},
};
use sha3::{Digest, Keccak256};

use super::traits::EIP712Encode;

pub struct Script {
    code_hash: [u8; 32],
    hash_type: String,
    args: Vec<u8>,
}

impl EIP712Encode for Script {
    fn type_name() -> &'static str {
        "Script"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"Script(bytes32 codeHash,string hashType,bytes args)");
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(self.code_hash.into())]));
        let hash_type: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(self.hash_type.as_bytes());
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(hash_type.into())]));
        let args: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(&self.args);
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(args.into())]));
    }
}

pub struct WithdrawalAsset {
    // CKB amount
    ckb_capacity: u64,
    // SUDT amount
    udt_amount: u128,
    udt_script_hash: [u8; 32],
}

impl EIP712Encode for WithdrawalAsset {
    fn type_name() -> &'static str {
        "WithdrawalAsset"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"WithdrawalAsset(uint256 ckbCapacity,uint256 UDTAmount,bytes32 UDTScriptHash)");
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(self.ckb_capacity.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.udt_amount.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.udt_script_hash.into())]));
    }
}

#[derive(Debug)]
pub enum AddressRegistry {
    ETH,
}

impl AddressRegistry {
    fn to_string(&self) -> &str {
        "ETH"
    }

    pub fn from_registry_id(registry_id: u32) -> Result<Self, Error> {
        match registry_id {
            gw_utils::gw_common::builtins::ETH_REGISTRY_ACCOUNT_ID => Ok(Self::ETH),
            _ => {
                debug!("Unsupported registry id : {}", registry_id);
                Err(Error::InvalidArgs)
            }
        }
    }
}

#[derive(Debug)]
pub struct RegistryAddress {
    registry: AddressRegistry,
    address: [u8; 20],
}

impl RegistryAddress {
    fn from_address(
        address: gw_utils::gw_common::registry_address::RegistryAddress,
    ) -> Result<Self, Error> {
        let registry = AddressRegistry::from_registry_id(address.registry_id)?;
        if address.address.len() != 20 {
            debug!(
                "Invalid ETH address len, expected 20, got {}",
                address.address.len()
            );
            return Err(Error::InvalidArgs);
        }
        Ok(RegistryAddress {
            registry,
            address: address.address.try_into().expect("eth address"),
        })
    }
}

impl EIP712Encode for RegistryAddress {
    fn type_name() -> &'static str {
        "RegistryAddress"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"RegistryAddress(string registry,address address)");
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        let registry: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(self.registry.to_string().as_bytes());
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(registry.into())]));
        buf.extend(ethabi::encode(&[Token::Address(self.address.into())]));
    }
}

/// L2Transaction
#[derive(Debug)]
pub struct L2Transaction {
    chain_id: u64,
    from: RegistryAddress,
    to: [u8; 32],
    nonce: u32,
    args: Vec<u8>,
}

impl EIP712Encode for L2Transaction {
    fn type_name() -> &'static str {
        "L2Transaction"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"L2Transaction(uint256 chainId,RegistryAddress from,bytes32 to,uint256 nonce,bytes args)");
        self.from.encode_type(buf);
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(self.chain_id.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.from.hash_struct().into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(self.to.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.nonce.into())]));
        let args: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(&self.args);
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(args.into())]));
    }
}

impl L2Transaction {
    pub fn from_raw(
        data: gw_utils::gw_types::packed::RawL2Transaction,
        sender_address: gw_utils::gw_common::registry_address::RegistryAddress,
        to_script_hash: gw_utils::gw_types::h256::H256,
    ) -> Result<Self, Error> {
        let sender_address = RegistryAddress::from_address(sender_address)?;
        let tx = L2Transaction {
            chain_id: data.chain_id().unpack(),
            nonce: data.nonce().unpack(),
            from: sender_address,
            to: to_script_hash.into(),
            args: data.args().unpack(),
        };
        Ok(tx)
    }
}

/// RawWithdrawalRequest
pub struct Withdrawal {
    address: RegistryAddress,
    nonce: u32,
    chain_id: u64,
    // withdrawal fee, paid to block producer
    fee: u128,
    // layer1 lock to withdraw after challenge period
    layer1_owner_lock: Script,
    // CKB amount
    withdraw: WithdrawalAsset,
}

impl EIP712Encode for Withdrawal {
    fn type_name() -> &'static str {
        "Withdrawal"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"Withdrawal(RegistryAddress address,uint256 nonce,uint256 chainId,uint256 fee,Script layer1OwnerLock,WithdrawalAsset withdraw)");
        self.address.encode_type(buf);
        self.layer1_owner_lock.encode_type(buf);
        self.withdraw.encode_type(buf);
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(
            self.address.hash_struct().into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(self.nonce.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.chain_id.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.fee.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.layer1_owner_lock.hash_struct().into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.withdraw.hash_struct().into(),
        )]));
    }
}

impl Withdrawal {
    pub fn from_raw(
        data: RawWithdrawalRequest,
        owner_lock: gw_utils::gw_types::packed::Script,
        address: gw_utils::gw_common::registry_address::RegistryAddress,
    ) -> Result<Self, Error> {
        let hash_type =
            match ScriptHashType::try_from(owner_lock.hash_type()).map_err(|hash_type| {
                debug!("Invalid hash type: {}", hash_type);
                Error::InvalidArgs
            })? {
                ScriptHashType::Data => "data",
                ScriptHashType::Type => "type",
            };
        let address = RegistryAddress::from_address(address)?;
        let withdrawal = Withdrawal {
            nonce: data.nonce().unpack(),
            address,
            withdraw: WithdrawalAsset {
                ckb_capacity: data.capacity().unpack(),
                udt_amount: data.amount().unpack(),
                udt_script_hash: data.sudt_script_hash().unpack(),
            },
            layer1_owner_lock: Script {
                code_hash: owner_lock.code_hash().unpack(),
                hash_type: hash_type.to_string(),
                args: owner_lock.args().unpack(),
            },
            fee: data.fee().unpack(),
            chain_id: data.chain_id().unpack(),
        };
        Ok(withdrawal)
    }
}

pub struct EIP712Domain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: Option<[u8; 20]>,
    pub salt: Option<[u8; 32]>,
}

impl EIP712Domain {
    pub fn domain_with_chain_id(chain_id: u64) -> EIP712Domain {
        EIP712Domain {
            name: "Godwoken".to_string(),
            chain_id,
            version: "1".to_string(),
            verifying_contract: None,
            salt: None,
        }
    }
}

impl EIP712Encode for EIP712Domain {
    fn type_name() -> &'static str {
        "EIP712Domain"
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"EIP712Domain(");
        buf.extend(b"string name,string version,uint256 chainId");
        if self.verifying_contract.is_some() {
            buf.extend(b",address verifyingContract");
        }
        if self.salt.is_some() {
            buf.extend(b",bytes32 salt");
        }
        buf.extend(b")");
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;

        let name: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(self.name.as_bytes());
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(name.into())]));
        let version: [u8; 32] = {
            let mut hasher = Keccak256::new();
            hasher.update(self.version.as_bytes());
            hasher.finalize().into()
        };
        buf.extend(ethabi::encode(&[Token::Uint(version.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.chain_id.into())]));
        if let Some(verifying_contract) = self.verifying_contract {
            buf.extend(ethabi::encode(&[Token::Address(verifying_contract.into())]));
        }
        if let Some(salt) = self.salt {
            buf.extend(ethabi::encode(&[Token::Uint(salt.into())]));
        }
    }
}
