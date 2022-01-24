use sha3::{Digest, Keccak256};

use super::traits::EIP712Encode;

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

pub struct Fee {
    udt_id: u32,
    udt_amount: u128,
}

impl EIP712Encode for Fee {
    fn type_name() -> String {
        "Fee".to_string()
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"Fee(uint256 UDTId,uint256 UDTAmount)");
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(self.udt_id.into())]));
        buf.extend(ethabi::encode(&[Token::Uint(self.udt_amount.into())]));
    }
}

// RawWithdrawalRequest
pub struct Withdrawal {
    account_script_hash: [u8; 32],
    // layer1 lock to withdraw after challenge period
    layer1_owner_lock: Script,
    // CKB amount
    withdraw: WithdrawalAsset,
    // withdrawal fee, paid to block producer
    fee: Fee,
}

impl EIP712Encode for Withdrawal {
    fn type_name() -> String {
        "Withdrawal".to_string()
    }

    fn encode_type(&self, buf: &mut Vec<u8>) {
        buf.extend(b"Withdrawal(bytes32 accountScriptHash,Script layer1OwnerLock,WithdrawalAsset withdraw,Fee fee)");
        self.fee.encode_type(buf);
        self.layer1_owner_lock.encode_type(buf);
        self.withdraw.encode_type(buf);
    }

    fn encode_data(&self, buf: &mut Vec<u8>) {
        use ethabi::Token;
        buf.extend(ethabi::encode(&[Token::Uint(
            self.account_script_hash.into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.layer1_owner_lock.hash_struct().into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.withdraw.hash_struct().into(),
        )]));
        buf.extend(ethabi::encode(&[Token::Uint(
            self.fee.hash_struct().into(),
        )]));
    }
}

pub struct EIP712Domain {
    name: String,
    version: String,
    chain_id: u64,
    verifying_contract: Option<[u8; 20]>,
    salt: Option<[u8; 32]>,
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
            types::{Fee, Script, Withdrawal, WithdrawalAsset},
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
    fn test_domain_seperator_encoding() {
        let domain_seperator = EIP712Domain {
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
        let domain_hash = domain_seperator.hash_struct();
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
            account_script_hash: hex::decode(
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            )
            .unwrap()
            .try_into()
            .unwrap(),
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
            fee: Fee {
                udt_id: 1,
                udt_amount: 100,
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
        let signature: [u8; 65] = hex::decode("ed612c71ae98ea3099a45f12f5a23548a25a66f66a2f16bd5fc3c8c905c1a6911199c6f62a8a24dafaf5105198b81f26d01d22c8a515c095e94a703259fc93f81c").unwrap().try_into().unwrap();
        let pubkey_hash = Secp256k1Eth::default()
            .recover(message.into(), &signature)
            .unwrap();
        assert_eq!(
            "898136badaf5fb2b8fb65ce832e7b6d13f89546a".to_string(),
            hex::encode(pubkey_hash)
        );
    }
}
