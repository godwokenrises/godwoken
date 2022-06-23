use std::convert::{TryFrom, TryInto};

use anyhow::{anyhow, bail, Result};
use gw_common::{builtins::ETH_REGISTRY_ACCOUNT_ID, H256};
use gw_types::{
    core::ScriptHashType,
    packed::{RawL2Transaction, RawWithdrawalRequest},
    prelude::Unpack,
};
use sha3::{Digest, Keccak256};

use super::traits::EIP712Encode;

#[derive(Debug)]
pub struct Script {
    code_hash: [u8; 32],
    hash_type: String,
    args: Vec<u8>,
}

impl EIP712Encode for Script {
    fn type_name() -> String {
        "Script".to_string()
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

#[derive(Debug)]
pub struct WithdrawalAsset {
    // CKB amount
    ckb_capacity: u64,
    // SUDT amount
    udt_amount: u128,
    udt_script_hash: [u8; 32],
}

impl EIP712Encode for WithdrawalAsset {
    fn type_name() -> String {
        "WithdrawalAsset".to_string()
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

    pub fn from_registry_id(registry_id: u32) -> Result<Self> {
        match registry_id {
            ETH_REGISTRY_ACCOUNT_ID => Ok(Self::ETH),
            _ => {
                bail!("Unsupported registry id : {}", registry_id)
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
    fn from_address(address: gw_common::registry_address::RegistryAddress) -> Result<Self> {
        let registry = AddressRegistry::from_registry_id(address.registry_id)?;
        if address.address.len() != 20 {
            bail!(
                "Invalid ETH address len, expected 20, got {}",
                address.address.len()
            );
        }
        Ok(RegistryAddress {
            registry,
            address: address.address.try_into().expect("eth address"),
        })
    }
}

impl EIP712Encode for RegistryAddress {
    fn type_name() -> String {
        "RegistryAddress".to_string()
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
    fn type_name() -> String {
        "L2Transaction".to_string()
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
        data: RawL2Transaction,
        sender_address: gw_common::registry_address::RegistryAddress,
        to_script_hash: H256,
    ) -> Result<Self> {
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
#[derive(Debug)]
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
    fn type_name() -> String {
        "Withdrawal".to_string()
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
        owner_lock: gw_types::packed::Script,
        address: gw_common::registry_address::RegistryAddress,
    ) -> Result<Self> {
        let hash_type = match ScriptHashType::try_from(owner_lock.hash_type())
            .map_err(|hash_type| anyhow!("Invalid hash type: {}", hash_type))?
        {
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

impl EIP712Encode for EIP712Domain {
    fn type_name() -> String {
        "EIP712Domain".to_string()
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

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use sha3::{Digest, Keccak256};

    use crate::account_lock_manage::{
        eip712::{
            traits::EIP712Encode,
            types::{
                AddressRegistry, L2Transaction, RegistryAddress, Script, Withdrawal,
                WithdrawalAsset,
            },
        },
        secp256k1::Secp256k1Eth,
        LockAlgorithm,
    };

    use super::EIP712Domain;

    struct Person {
        name: String,
        wallet: [u8; 20],
    }

    impl EIP712Encode for Person {
        fn type_name() -> String {
            "Person".to_string()
        }

        fn encode_type(&self, buf: &mut Vec<u8>) {
            buf.extend(b"Person(string name,address wallet)");
        }

        fn encode_data(&self, buf: &mut Vec<u8>) {
            use ethabi::Token;

            let name: [u8; 32] = {
                let mut hasher = Keccak256::new();
                hasher.update(self.name.as_bytes());
                hasher.finalize().into()
            };
            buf.extend(ethabi::encode(&[Token::Uint(name.into())]));
            buf.extend(ethabi::encode(&[Token::Address(self.wallet.into())]));
        }
    }

    struct Mail {
        from: Person,
        to: Person,
        contents: String,
    }

    impl EIP712Encode for Mail {
        fn type_name() -> String {
            "Mail".to_string()
        }

        fn encode_type(&self, buf: &mut Vec<u8>) {
            buf.extend(b"Mail(Person from,Person to,string contents)");
            self.from.encode_type(buf);
        }

        fn encode_data(&self, buf: &mut Vec<u8>) {
            use ethabi::Token;

            // self.from.encode_data(buf);
            // self.to.encode_data(buf);
            buf.extend(ethabi::encode(&[Token::Uint(
                self.from.hash_struct().into(),
            )]));
            buf.extend(ethabi::encode(&[Token::Uint(self.to.hash_struct().into())]));

            let contents: [u8; 32] = {
                let mut hasher = Keccak256::new();
                hasher.update(self.contents.as_bytes());
                hasher.finalize().into()
            };
            buf.extend(ethabi::encode(&[Token::Uint(contents.into())]));
        }
    }

    #[test]
    fn test_domain_separator_encoding() {
        let domain_separator = EIP712Domain {
            name: "Ether Mail".to_string(),
            version: "1".to_string(),
            chain_id: 1,
            verifying_contract: {
                Some(
                    hex::decode("CcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC")
                        .unwrap()
                        .try_into()
                        .unwrap(),
                )
            },
            salt: None,
        };
        let domain_hash = domain_separator.hash_struct();
        assert_eq!(
            hex::encode(domain_hash),
            "f2cee375fa42b42143804025fc449deafd50cc031ca257e0b194a650a912090f"
        )
    }

    #[test]
    fn test_sign_message() {
        let mail = Mail {
            from: Person {
                name: "Cow".to_string(),
                wallet: hex::decode("CD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            },
            to: Person {
                name: "Bob".to_string(),
                wallet: hex::decode("bBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB")
                    .unwrap()
                    .try_into()
                    .unwrap(),
            },
            contents: "Hello, Bob!".to_string(),
        };
        let hash = mail.hash_struct();
        assert_eq!(
            hex::encode(hash),
            "c52c0ee5d84264471806290a3f2c4cecfc5490626bf912d01f240d7a274b371e"
        );

        // verify EIP 712 signature
        let message = mail.eip712_message(
            hex::decode("f2cee375fa42b42143804025fc449deafd50cc031ca257e0b194a650a912090f")
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let signature = {
            let r = hex::decode("4355c47d63924e8a72e509b65029052eb6c299d53a04e167c5775fd466751c9d")
                .unwrap();
            let s = hex::decode("07299936d304c153f6443dfa05f40ff007d72911b6f72307f996231605b91562")
                .unwrap();
            let v = 28;
            let mut buf = [0u8; 65];
            buf[..32].copy_from_slice(&r);
            buf[32..64].copy_from_slice(&s);
            buf[64] = v;
            buf
        };
        let pubkey_hash = Secp256k1Eth::default()
            .recover(message.into(), &signature)
            .unwrap();
        assert_eq!(hex::encode(mail.from.wallet), hex::encode(pubkey_hash));
    }

    #[test]
    fn test_sign_withdrawal_message() {
        let withdrawal = Withdrawal {
            address: RegistryAddress {
                registry: AddressRegistry::ETH,
                address: hex::decode("dddddddddddddddddddddddddddddddddddddddd")
                    .unwrap()
                    .try_into()
                    .expect("address"),
            },
            nonce: 1,
            chain_id: 1,
            fee: 1000u64.into(),
            layer1_owner_lock: Script {
                code_hash: hex::decode(
                    "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                )
                .unwrap()
                .try_into()
                .unwrap(),
                hash_type: "type".to_string(),
                args: hex::decode("1234").unwrap(),
            },
            withdraw: WithdrawalAsset {
                ckb_capacity: 1000,
                udt_amount: 300,
                udt_script_hash: hex::decode(
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                )
                .unwrap()
                .try_into()
                .unwrap(),
            },
        };

        // verify EIP 712 signature
        let domain_seperator = EIP712Domain {
            name: "Godwoken".to_string(),
            version: "1".to_string(),
            chain_id: 1,
            verifying_contract: None,
            salt: None,
        };
        let message = withdrawal.eip712_message(domain_seperator.hash_struct());
        let signature: [u8; 65] = hex::decode("22cae59f1bfaf58f423d1a414cbcaefd45a89dd54c9142fccbb2473c74f4741b45f77f1f3680b8c0b6362957c8d79f96a683a859ccbf22a6cfc1ebc311b936d301").unwrap().try_into().unwrap();
        let pubkey_hash = Secp256k1Eth::default()
            .recover(message.into(), &signature)
            .unwrap();
        assert_eq!(
            "cc3e7fb0176a0e22a7f675306ceeb61d26eb0dc4".to_string(),
            hex::encode(pubkey_hash)
        );
    }

    #[test]
    fn test_l2_transaction() {
        let tx = L2Transaction {
            chain_id: 1,
            from: RegistryAddress {
                registry: AddressRegistry::ETH,
                address: hex::decode("e8ae579256c3b84efb76bbb69cb6bcbef1375f00")
                    .unwrap()
                    .try_into()
                    .expect("address"),
            },
            to: hex::decode("ae39eea37dfa6b41004c50efddeb6747f72bb25ea174b2a68bd4eafc641e7c3e")
                .unwrap()
                .try_into()
                .expect("script hash"),
            nonce: 9,
            args: Default::default(),
        };

        // verify EIP 712 signature
        let domain_seperator = EIP712Domain {
            name: "Godwoken".to_string(),
            version: "1".to_string(),
            chain_id: 1,
            verifying_contract: None,
            salt: None,
        };
        let message = tx.eip712_message(domain_seperator.hash_struct());
        let signature: [u8; 65] = hex::decode("64b164f5303000c283119974d7ba8f050cc7429984af904134d5cda6d3ce045934cc6b6f513ec939c2ae4cfb9cbee249ba8ae86f6274e4035c150f9c8e634a3a1b").unwrap().try_into().unwrap();
        let pubkey_hash = Secp256k1Eth::default()
            .recover(message.into(), &signature)
            .unwrap();
        assert_eq!(
            "e8ae579256c3b84efb76bbb69cb6bcbef1375f00".to_string(),
            hex::encode(pubkey_hash)
        );
    }
}
