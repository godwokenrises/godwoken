use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::str::FromStr;

use bech32::{self, convert_bits, ToBase32, Variant};
use ckb_hash::blake2b_256;
use ckb_types::{
    bytes::Bytes,
    core::ScriptHashType,
    packed::{Byte32, Script},
    prelude::*,
    H160, H256,
};

use super::NetworkType;
use crate::utils::sdk::constants::{
    ACP_TYPE_HASH_AGGRON, ACP_TYPE_HASH_LINA, MULTISIG_TYPE_HASH, SIGHASH_TYPE_HASH,
};
pub use old_addr::{Address as OldAddress, AddressFormat as OldAddressFormat};

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
pub enum AddressType {
    // full version identifies the hash_type and vm_version
    Full = 0x00,
    // short version for locks with popular code_hash, deprecated
    Short = 0x01,
    // full version with hash_type = "Data", deprecated
    FullData = 0x02,
    // full version with hash_type = "Type", deprecated
    FullType = 0x04,
}

impl AddressType {
    pub fn from_u8(value: u8) -> Result<AddressType, String> {
        match value {
            0x00 => Ok(AddressType::Full),
            0x01 => Ok(AddressType::Short),
            0x02 => Ok(AddressType::FullData),
            0x04 => Ok(AddressType::FullType),
            _ => Err(format!("Invalid address type value: {}", value)),
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(u8)]
pub enum CodeHashIndex {
    /// SECP256K1 + blake160, args: `blake160(PK)`
    Sighash = 0x00,
    /// SECP256K1 + multisig, args: `multisig script hash`
    Multisig = 0x01,
    /// anyone_can_pay, args: `blake160(PK)`
    Acp = 0x02,
}

impl CodeHashIndex {
    pub fn from_u8(value: u8) -> Result<CodeHashIndex, String> {
        match value {
            0x00 => Ok(CodeHashIndex::Sighash),
            0x01 => Ok(CodeHashIndex::Multisig),
            0x02 => Ok(CodeHashIndex::Acp),
            _ => Err(format!("Invalid code hash index value: {}", value)),
        }
    }
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub enum AddressPayload {
    // Remain the address format before ckb2021.
    Short {
        index: CodeHashIndex,
        hash: H160,
    },
    Full {
        hash_type: ScriptHashType,
        code_hash: Byte32,
        args: Bytes,
    },
}

impl AddressPayload {
    pub fn new_short(index: CodeHashIndex, hash: H160) -> AddressPayload {
        AddressPayload::Short { index, hash }
    }

    pub fn new_full(hash_type: ScriptHashType, code_hash: Byte32, args: Bytes) -> AddressPayload {
        AddressPayload::Full {
            hash_type,
            code_hash,
            args,
        }
    }
    #[deprecated(since = "0.100.0-rc5", note = "Use AddressType::Full instead")]
    pub fn new_full_data(code_hash: Byte32, args: Bytes) -> AddressPayload {
        Self::new_full(ScriptHashType::Data, code_hash, args)
    }
    #[deprecated(since = "0.100.0-rc5", note = "Use AddressType::Full instead")]
    pub fn new_full_type(code_hash: Byte32, args: Bytes) -> AddressPayload {
        Self::new_full(ScriptHashType::Type, code_hash, args)
    }

    pub fn ty(&self, is_new: bool) -> AddressType {
        match self {
            AddressPayload::Short { .. } => AddressType::Short,
            AddressPayload::Full { hash_type, .. } => match (hash_type, is_new) {
                (ScriptHashType::Data, true) => AddressType::Full,
                (ScriptHashType::Type, true) => AddressType::Full,
                (ScriptHashType::Data1, _) => AddressType::Full,
                (ScriptHashType::Data, false) => AddressType::FullData,
                (ScriptHashType::Type, false) => AddressType::FullType,
            },
        }
    }

    pub fn is_short(&self) -> bool {
        matches!(self, AddressPayload::Short { .. })
    }
    pub fn is_short_acp(&self) -> bool {
        matches!(
            self,
            AddressPayload::Short {
                index: CodeHashIndex::Acp,
                ..
            }
        )
    }

    pub fn hash_type(&self) -> ScriptHashType {
        match self {
            AddressPayload::Short { .. } => ScriptHashType::Type,
            AddressPayload::Full { hash_type, .. } => *hash_type,
        }
    }

    /// Get the code hash of an address
    ///
    /// # Panics
    ///
    /// When current addres is short format anyone-can-pay address, and the
    /// network type is not `Mainnet` or `Testnet` this function will panic.
    pub fn code_hash(&self, network: Option<NetworkType>) -> Byte32 {
        match self {
            AddressPayload::Short { index, .. } => match index {
                CodeHashIndex::Sighash => SIGHASH_TYPE_HASH.clone().pack(),
                CodeHashIndex::Multisig => MULTISIG_TYPE_HASH.clone().pack(),
                CodeHashIndex::Acp => match network {
                    Some(NetworkType::Mainnet) => ACP_TYPE_HASH_LINA.clone().pack(),
                    Some(NetworkType::Testnet) => ACP_TYPE_HASH_AGGRON.clone().pack(),
                    _ => panic!("network type must be `mainnet` or `testnet` when handle short format anyone-can-pay address"),
                }
            },
            AddressPayload::Full { code_hash, .. } => code_hash.clone(),
        }
    }

    pub fn args(&self) -> Bytes {
        match self {
            AddressPayload::Short { hash, .. } => Bytes::from(hash.as_bytes().to_vec()),
            AddressPayload::Full { args, .. } => args.clone(),
        }
    }

    pub fn from_pubkey(pubkey: &secp256k1::PublicKey) -> AddressPayload {
        // Serialize pubkey as compressed format
        let hash = H160::from_slice(&blake2b_256(&pubkey.serialize()[..])[0..20])
            .expect("Generate hash(H160) from pubkey failed");
        AddressPayload::from_pubkey_hash(hash)
    }

    pub fn from_pubkey_hash(hash: H160) -> AddressPayload {
        let index = CodeHashIndex::Sighash;
        AddressPayload::Short { index, hash }
    }

    pub fn display_with_network(&self, network: NetworkType, is_new: bool) -> String {
        let hrp = network.to_prefix();
        let (data, variant) = if is_new {
            // payload = 0x00 | code_hash | hash_type | args
            let code_hash = self.code_hash(Some(network));
            let hash_type = self.hash_type();
            let args = self.args();
            let mut data = vec![0u8; 34 + args.len()];
            data[0] = 0x00;
            data[1..33].copy_from_slice(code_hash.as_slice());
            data[33] = hash_type as u8;
            data[34..].copy_from_slice(args.as_ref());
            (data, bech32::Variant::Bech32m)
        } else {
            match self {
                // payload = 0x01 | code_hash_index | args
                AddressPayload::Short { index, hash } => {
                    let mut data = vec![0u8; 22];
                    data[0] = 0x01;
                    data[1] = (*index) as u8;
                    data[2..].copy_from_slice(hash.as_bytes());
                    // short address always use bech32
                    (data, bech32::Variant::Bech32)
                }
                AddressPayload::Full {
                    code_hash, args, ..
                } => {
                    // payload = 0x02/0x04 | code_hash | args
                    let mut data = vec![0u8; 33 + args.len()];
                    data[0] = self.ty(false) as u8;
                    data[1..33].copy_from_slice(code_hash.as_slice());
                    data[33..].copy_from_slice(args.as_ref());
                    (data, bech32::Variant::Bech32)
                }
            }
        };
        bech32::encode(hrp, data.to_base32(), variant)
            .unwrap_or_else(|_| panic!("Encode address failed: payload={:?}", self))
    }
}

impl fmt::Debug for AddressPayload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hash_type = match self.hash_type() {
            ScriptHashType::Type => "type",
            ScriptHashType::Data => "data",
            ScriptHashType::Data1 => "data1",
        };
        f.debug_struct("AddressPayload")
            .field("hash_type", &hash_type)
            .field("code_hash", &self.code_hash(None))
            .field("args", &self.args())
            .finish()
    }
}

impl From<&AddressPayload> for Script {
    fn from(payload: &AddressPayload) -> Script {
        Script::new_builder()
            .hash_type(payload.hash_type().into())
            .code_hash(payload.code_hash(None))
            .args(payload.args().pack())
            .build()
    }
}

impl From<Script> for AddressPayload {
    #[allow(clippy::fallible_impl_from)]
    fn from(lock: Script) -> AddressPayload {
        let hash_type: ScriptHashType = lock.hash_type().try_into().expect("Invalid hash_type");
        let code_hash = lock.code_hash();
        let code_hash_h256: H256 = code_hash.unpack();
        let args = lock.args().raw_data();
        if hash_type == ScriptHashType::Type
            && code_hash_h256 == SIGHASH_TYPE_HASH
            && args.len() == 20
        {
            let index = CodeHashIndex::Sighash;
            let hash = H160::from_slice(args.as_ref()).unwrap();
            AddressPayload::Short { index, hash }
        } else if hash_type == ScriptHashType::Type
            && code_hash_h256 == MULTISIG_TYPE_HASH
            && args.len() == 20
        {
            let index = CodeHashIndex::Multisig;
            let hash = H160::from_slice(args.as_ref()).unwrap();
            AddressPayload::Short { index, hash }
        } else if hash_type == ScriptHashType::Type
            && (code_hash_h256 == ACP_TYPE_HASH_LINA || code_hash_h256 == ACP_TYPE_HASH_AGGRON)
            && args.len() == 20
        {
            // NOTE: anoney-can-pay script args can larger than 20 bytes, here
            // args.len() != 20 is not a short format address, see RFC21 for
            // more details.
            let index = CodeHashIndex::Acp;
            let hash = H160::from_slice(args.as_ref()).unwrap();
            AddressPayload::Short { index, hash }
        } else {
            AddressPayload::Full {
                hash_type,
                code_hash,
                args,
            }
        }
    }
}

#[derive(Hash, Eq, PartialEq, Clone)]
pub struct Address {
    network: NetworkType,
    payload: AddressPayload,
    is_new: bool,
}

impl Address {
    pub fn new(network: NetworkType, payload: AddressPayload, is_new: bool) -> Address {
        Address {
            network,
            payload,
            is_new,
        }
    }
    /// The network type of current address
    pub fn network(&self) -> NetworkType {
        self.network
    }
    /// The address payload
    pub fn payload(&self) -> &AddressPayload {
        &self.payload
    }
    /// If true the address is ckb2021 format
    pub fn is_new(&self) -> bool {
        self.is_new
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hash_type = match self.payload.hash_type() {
            ScriptHashType::Type => "type",
            ScriptHashType::Data => "data",
            ScriptHashType::Data1 => "data1",
        };
        f.debug_struct("Address")
            .field("network", &self.network)
            .field("hash_type", &hash_type)
            .field("code_hash", &self.payload.code_hash(Some(self.network)))
            .field("args", &self.payload.args())
            .field("is_new", &self.is_new)
            .finish()
    }
}

impl From<&Address> for Script {
    fn from(addr: &Address) -> Script {
        Script::new_builder()
            .hash_type(addr.payload.hash_type().into())
            .code_hash(addr.payload.code_hash(Some(addr.network)))
            .args(addr.payload.args().pack())
            .build()
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "{}",
            self.payload.display_with_network(self.network, self.is_new)
        )
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (hrp, data, variant) = bech32::decode(input).map_err(|err| err.to_string())?;
        let network =
            NetworkType::from_prefix(&hrp).ok_or_else(|| format!("Invalid hrp: {}", hrp))?;
        let data = convert_bits(&data, 5, 8, false).unwrap();
        let ty = AddressType::from_u8(data[0])?;
        match ty {
            // payload = 0x01 | code_hash_index | args
            AddressType::Short => {
                if variant != Variant::Bech32 {
                    return Err("short address must use bech32 encoding".to_string());
                }
                if data.len() != 22 {
                    return Err(format!("Invalid input data length {}", data.len()));
                }
                let index = CodeHashIndex::from_u8(data[1])?;
                let hash = H160::from_slice(&data[2..22]).unwrap();
                let payload = AddressPayload::Short { index, hash };
                Ok(Address {
                    network,
                    payload,
                    is_new: false,
                })
            }
            // payload = 0x02/0x04 | code_hash | args
            AddressType::FullData | AddressType::FullType => {
                if variant != Variant::Bech32 {
                    return Err(
                        "non-ckb2021 format full address must use bech32 encoding".to_string()
                    );
                }
                if data.len() < 33 {
                    return Err(format!("Insufficient data length: {}", data.len()));
                }
                let hash_type = if ty == AddressType::FullData {
                    ScriptHashType::Data
                } else {
                    ScriptHashType::Type
                };
                let code_hash = Byte32::from_slice(&data[1..33]).unwrap();
                let args = Bytes::from(data[33..].to_vec());
                let payload = AddressPayload::Full {
                    hash_type,
                    code_hash,
                    args,
                };
                Ok(Address {
                    network,
                    payload,
                    is_new: false,
                })
            }
            // payload = 0x00 | code_hash | hash_type | args
            AddressType::Full => {
                if variant != Variant::Bech32m {
                    return Err("ckb2021 format full address must use bech32m encoding".to_string());
                }
                if data.len() < 34 {
                    return Err(format!("Insufficient data length: {}", data.len()));
                }
                let code_hash = Byte32::from_slice(&data[1..33]).unwrap();
                let hash_type =
                    ScriptHashType::try_from(data[33]).map_err(|err| err.to_string())?;
                let args = Bytes::from(data[34..].to_vec());
                let payload = AddressPayload::Full {
                    hash_type,
                    code_hash,
                    args,
                };
                Ok(Address {
                    network,
                    payload,
                    is_new: true,
                })
            }
        }
    }
}

mod old_addr {
    use super::{
        bech32, blake2b_256, convert_bits, Deserialize, NetworkType, Script, ScriptHashType,
        Serialize, ToBase32, H160, H256,
    };
    use ckb_crypto::secp::Pubkey;
    use ckb_types::prelude::*;

    // \x01 is the P2PH version
    const P2PH_MARK: &[u8] = b"\x01P2PH";

    #[derive(Hash, Eq, PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
    pub enum AddressFormat {
        // SECP256K1 algorithm	PK
        #[allow(dead_code)]
        Sp2k,
        // SECP256R1 algorithm	PK
        #[allow(dead_code)]
        Sp2r,
        // SECP256K1 + blake160	blake160(pk)
        P2ph,
        // Alias of SP2K	PK
        #[allow(dead_code)]
        P2pk,
    }

    impl Default for AddressFormat {
        fn default() -> AddressFormat {
            AddressFormat::P2ph
        }
    }

    impl AddressFormat {
        pub fn from_bytes(format: &[u8]) -> Result<AddressFormat, String> {
            match format {
                P2PH_MARK => Ok(AddressFormat::P2ph),
                _ => Err(format!("Unsupported address format data: {:?}", format)),
            }
        }

        pub fn to_bytes(self) -> Result<Vec<u8>, String> {
            match self {
                AddressFormat::P2ph => Ok(P2PH_MARK.to_vec()),
                _ => Err(format!("Unsupported address format: {:?}", self)),
            }
        }
    }

    #[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
    pub struct Address {
        format: AddressFormat,
        hash: H160,
    }

    impl Address {
        pub fn new_default(hash: H160) -> Address {
            let format = AddressFormat::P2ph;
            Address { format, hash }
        }

        pub fn hash(&self) -> &H160 {
            &self.hash
        }

        pub fn lock_script(&self, code_hash: H256) -> Script {
            Script::new_builder()
                .args(self.hash.as_bytes().pack())
                .code_hash(code_hash.pack())
                .hash_type(ScriptHashType::Data.into())
                .build()
        }

        pub fn from_pubkey(format: AddressFormat, pubkey: &Pubkey) -> Result<Address, String> {
            if format != AddressFormat::P2ph {
                return Err("Only support P2PH for now".to_owned());
            }
            // Serialize pubkey as compressed format
            let hash = H160::from_slice(&blake2b_256(pubkey.serialize())[0..20])
                .expect("Generate hash(H160) from pubkey failed");
            Ok(Address { format, hash })
        }

        pub fn from_lock_arg(bytes: &[u8]) -> Result<Address, String> {
            let format = AddressFormat::P2ph;
            let hash = H160::from_slice(bytes).map_err(|err| err.to_string())?;
            Ok(Address { format, hash })
        }

        pub fn from_input(network: NetworkType, input: &str) -> Result<Address, String> {
            let (hrp, data, _variant) = bech32::decode(input).map_err(|err| err.to_string())?;
            if NetworkType::from_prefix(&hrp)
                .filter(|input_network| input_network == &network)
                .is_none()
            {
                return Err(format!("Invalid hrp({}) for {}", hrp, network));
            }
            let data = convert_bits(&data, 5, 8, false).unwrap();
            if data.len() != 25 {
                return Err(format!("Invalid input data length {}", data.len()));
            }
            let format = AddressFormat::from_bytes(&data[0..5])?;
            let hash = H160::from_slice(&data[5..25]).map_err(|err| err.to_string())?;
            Ok(Address { format, hash })
        }

        pub fn display_with_prefix(&self, network: NetworkType) -> String {
            let hrp = network.to_prefix();
            let mut data = [0; 25];
            let format_data = self.format.to_bytes().expect("Invalid address format");
            data[0..5].copy_from_slice(&format_data[0..5]);
            data[5..25].copy_from_slice(self.hash.as_bytes());
            bech32::encode(hrp, data.to_base32(), bech32::Variant::Bech32)
                .unwrap_or_else(|_| panic!("Encode address failed: hash={:?}", self.hash))
        }

        #[allow(clippy::inherent_to_string)]
        #[deprecated(
            since = "0.25.0",
            note = "Name conflicts with the inherent to_string method. Use display_with_prefix instead."
        )]
        pub fn to_string(&self, network: NetworkType) -> String {
            self.display_with_prefix(network)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ckb_types::{h160, h256};

    #[test]
    fn test_short_address() {
        let payload =
            AddressPayload::from_pubkey_hash(h160!("0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64"));
        let address = Address::new(NetworkType::Mainnet, payload, false);
        assert_eq!(
            address.to_string(),
            "ckb1qyqt8xaupvm8837nv3gtc9x0ekkj64vud3jqfwyw5v"
        );
        assert_eq!(
            address,
            Address::from_str("ckb1qyqt8xaupvm8837nv3gtc9x0ekkj64vud3jqfwyw5v").unwrap()
        );

        let payload =
            AddressPayload::from_pubkey_hash(h160!("0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64"));
        let address = Address::new(NetworkType::Mainnet, payload.clone(), false);
        let address_new = Address::new(NetworkType::Mainnet, payload, true);
        assert_eq!(
            address.to_string(),
            "ckb1qyqt8xaupvm8837nv3gtc9x0ekkj64vud3jqfwyw5v"
        );
        assert_eq!(
            address_new.to_string(),
            "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4"
        );

        let index = CodeHashIndex::Multisig;
        let payload =
            AddressPayload::new_short(index, h160!("0x4fb2be2e5d0c1a3b8694f832350a33c1685d477a"));
        let address = Address::new(NetworkType::Mainnet, payload, false);
        assert_eq!(
            address.to_string(),
            "ckb1qyq5lv479ewscx3ms620sv34pgeuz6zagaaqklhtgg"
        );
        assert_eq!(
            address,
            Address::from_str("ckb1qyq5lv479ewscx3ms620sv34pgeuz6zagaaqklhtgg").unwrap()
        );
    }

    #[test]
    fn test_old_full_address() {
        let hash_type = ScriptHashType::Type;
        let code_hash = Byte32::from_slice(
            h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").as_bytes(),
        )
        .unwrap();
        let args = Bytes::from(h160!("0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64").as_bytes());
        let payload = AddressPayload::new_full(hash_type, code_hash, args);
        let address = Address::new(NetworkType::Mainnet, payload, false);
        assert_eq!(address.to_string(), "ckb1qjda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xw3vumhs9nvu786dj9p0q5elx66t24n3kxgj53qks");
        assert_eq!(address, Address::from_str("ckb1qjda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xw3vumhs9nvu786dj9p0q5elx66t24n3kxgj53qks").unwrap());
    }

    #[test]
    fn test_new_full_address() {
        let code_hash = Byte32::from_slice(
            h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").as_bytes(),
        )
        .unwrap();
        let args = Bytes::from(h160!("0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64").as_bytes());

        let payload =
            AddressPayload::new_full(ScriptHashType::Type, code_hash.clone(), args.clone());
        let address = Address::new(NetworkType::Mainnet, payload, true);
        assert_eq!(address.to_string(), "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4");
        assert_eq!(address, Address::from_str("ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4").unwrap());

        let payload =
            AddressPayload::new_full(ScriptHashType::Data, code_hash.clone(), args.clone());
        let address = Address::new(NetworkType::Mainnet, payload, true);
        assert_eq!(address.to_string(), "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl");
        assert_eq!(address, Address::from_str("ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq9nnw7qkdnnclfkg59uzn8umtfd2kwxceqvguktl").unwrap());

        let payload = AddressPayload::new_full(ScriptHashType::Data1, code_hash, args);
        let address = Address::new(NetworkType::Mainnet, payload, true);
        assert_eq!(address.to_string(), "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4nnw7qkdnnclfkg59uzn8umtfd2kwxceqcydzyt");
        assert_eq!(address, Address::from_str("ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq4nnw7qkdnnclfkg59uzn8umtfd2kwxceqcydzyt").unwrap());
    }

    #[test]
    fn test_parse_display_address() {
        let addr_str = "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqgvf0k9sc40s3azmpfvhyuudhahpsj72tsr8cx3d";
        let addr = Address::from_str(addr_str).unwrap();
        assert_eq!(addr.to_string(), addr_str);
        assert_eq!(
            addr.payload().code_hash(None).as_slice(),
            h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8").as_bytes()
        );
        assert_eq!(addr.payload().hash_type(), ScriptHashType::Type);
        assert_eq!(
            addr.payload().args().as_ref(),
            hex::decode("0c4bec5862af847a2d852cb939c6dfb70c25e52e").unwrap()
        );
    }

    #[test]
    fn test_invalid_short_address() {
        // INVALID bech32 encoding
        {
            let mut data = vec![0u8; 22];
            data[0] = 0x01;
            data[1] = CodeHashIndex::Sighash as u8;
            data[2..]
                .copy_from_slice(h160!("0x4fb2be2e5d0c1a3b8694f832350a33c1685d477a").as_bytes());
            let variant = bech32::Variant::Bech32m;
            let addr = bech32::encode("ckb", data.to_base32(), variant).unwrap();
            let expected_addr = "ckb1qyqylv479ewscx3ms620sv34pgeuz6zagaaqh0knz7";
            assert_eq!(addr, expected_addr);
            assert_eq!(
                Address::from_str(expected_addr),
                Err("short address must use bech32 encoding".to_string())
            );
        }
        // INVALID data length
        {
            let mut data = vec![0u8; 23];
            data[0] = 0x01;
            data[1] = CodeHashIndex::Sighash as u8;
            data[2..].copy_from_slice(
                &hex::decode("4fb2be2e5d0c1a3b8694f832350a33c1685d477a33").unwrap(),
            );
            let variant = bech32::Variant::Bech32;
            let addr = bech32::encode("ckb", data.to_base32(), variant).unwrap();
            let expected_addr = "ckb1qyqylv479ewscx3ms620sv34pgeuz6zagaarxdzvx03";
            assert_eq!(addr, expected_addr);
            assert_eq!(
                Address::from_str(expected_addr),
                Err("Invalid input data length 23".to_string())
            );
        }
        // INVALID code hash index
        {
            let mut data = vec![0u8; 22];
            data[0] = 0x01;
            data[1] = 17;
            data[2..]
                .copy_from_slice(h160!("0x4fb2be2e5d0c1a3b8694f832350a33c1685d477a").as_bytes());
            let variant = bech32::Variant::Bech32;
            let addr = bech32::encode("ckb", data.to_base32(), variant).unwrap();
            let expected_addr = "ckb1qyg5lv479ewscx3ms620sv34pgeuz6zagaaqajch0c";
            assert_eq!(addr, expected_addr);
            assert_eq!(
                Address::from_str(expected_addr),
                Err("Invalid code hash index value: 17".to_string())
            );
        }
    }

    #[test]
    fn test_invalid_old_full_address() {
        // INVALID bech32 encoding
        {
            let args = hex::decode("4fb2be2e5d0c1a3b86").unwrap();
            let mut data = vec![0u8; 33 + args.len()];
            data[0] = AddressType::FullData as u8;
            data[1..33].copy_from_slice(
                h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                    .as_bytes(),
            );
            data[33..].copy_from_slice(args.as_ref());
            let variant = bech32::Variant::Bech32m;
            let addr = bech32::encode("ckb", data.to_base32(), variant).unwrap();
            let expected_addr =
                "ckb1q2da0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsnajhch96rq68wrqn2tmhm";
            assert_eq!(addr, expected_addr);
            assert_eq!(
                Address::from_str(expected_addr),
                Err("non-ckb2021 format full address must use bech32 encoding".to_string())
            );
        }
    }

    #[test]
    fn test_invalid_new_address() {
        // INVALID bech32 encoding
        for (hash_type, expected_addr) in [
            (
                ScriptHashType::Type,
                "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq20k2lzuhgvrgacv4tmr88",
            ),
            (
                ScriptHashType::Data,
                "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqz0k2lzuhgvrgacvhcym08",
            ),
            (
                ScriptHashType::Data1,
                "ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqj0k2lzuhgvrgacvnhnzl8",
            ),
        ] {
            let code_hash =
                h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8");
            let args = hex::decode("4fb2be2e5d0c1a3b86").unwrap();
            let mut data = vec![0u8; 34 + args.len()];
            data[0] = 0x00;
            data[1..33].copy_from_slice(code_hash.as_bytes());
            data[33] = hash_type as u8;
            data[34..].copy_from_slice(args.as_ref());
            let variant = bech32::Variant::Bech32;
            let addr = bech32::encode("ckb", data.to_base32(), variant).unwrap();
            assert_eq!(addr, expected_addr);
            assert_eq!(
                Address::from_str(expected_addr),
                Err("ckb2021 format full address must use bech32m encoding".to_string())
            );
        }
    }

    #[test]
    fn test_address_debug() {
        let payload = AddressPayload::Full {
            hash_type: ScriptHashType::Data1,
            code_hash: h256!("0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8")
                .pack(),
            args: Bytes::from("abcd"),
        };
        let address = Address::new(NetworkType::Mainnet, payload.clone(), true);
        assert_eq!(format!("{:?}", payload), "AddressPayload { hash_type: \"data1\", code_hash: Byte32(0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8), args: b\"abcd\" }");
        assert_eq!(format!("{:?}", address), "Address { network: Mainnet, hash_type: \"data1\", code_hash: Byte32(0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8), args: b\"abcd\", is_new: true }");
    }
}
