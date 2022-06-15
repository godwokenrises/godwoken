use gw_types::{bytes::Bytes, packed::RawL2Transaction, prelude::*};

pub struct PolyjuiceParser(Bytes);

impl PolyjuiceParser {
    pub fn from_raw_l2_tx(raw_tx: &RawL2Transaction) -> Option<Self> {
        let args: Bytes = raw_tx.args().unpack();
        let args_len = args.len();
        if args_len < 52 {
            return None;
        }
        if args[0..7] != b"\xFF\xFF\xFFPOLY"[..] {
            return None;
        }
        let parser = Self(args);
        // check data size
        if args_len != 52 + parser.data_size() {
            return None;
        }
        Some(parser)
    }

    pub fn gas(&self) -> u64 {
        let mut data = [0u8; 8];
        data.copy_from_slice(&self.0[8..16]);
        u64::from_le_bytes(data)
    }

    pub fn gas_price(&self) -> u128 {
        let mut data = [0u8; 16];
        data.copy_from_slice(&self.0[16..32]);
        u128::from_le_bytes(data)
    }

    pub fn is_create(&self) -> bool {
        // 3 for EVMC_CREATE
        self.0[7] == 3
    }

    pub fn value(&self) -> u128 {
        let mut data = [0u8; 16];
        data.copy_from_slice(&self.0[32..48]);
        u128::from_le_bytes(data)
    }

    pub fn data_size(&self) -> usize {
        let mut data = [0u8; 4];
        data.copy_from_slice(&self.0[48..52]);
        u32::from_le_bytes(data) as usize
    }

    pub fn data(&self) -> &[u8] {
        &self.0[52..52 + self.data_size()]
    }
}
