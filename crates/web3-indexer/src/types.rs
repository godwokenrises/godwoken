use gw_common::H256;
use gw_types::U256;
use sha3::{Digest, Keccak256};
use sqlx::types::chrono::{DateTime, Utc};

type Address = [u8; 20];

#[derive(Debug)]
pub struct Block {
    pub number: u64,
    pub hash: H256,
    pub parent_hash: H256,
    pub logs_bloom: Vec<u8>,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub miner: Address,
    pub size: usize,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug)]
pub struct Transaction {
    pub gw_tx_hash: H256,
    pub chain_id: Option<u64>,
    pub block_number: u64,
    pub block_hash: H256,
    pub transaction_index: u32,
    pub from_address: Address,
    pub to_address: Option<Address>,
    pub value: U256,
    pub nonce: u32,
    pub gas_limit: U256,
    pub gas_price: u128,
    pub data: Vec<u8>,
    pub v: u64,
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub cumulative_gas_used: U256,
    pub gas_used: U256,
    pub logs_bloom: Vec<u8>,
    pub contract_address: Option<Address>,
    pub status: bool,
}

impl Transaction {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gw_tx_hash: H256,
        chain_id: Option<u64>,
        block_number: u64,
        block_hash: H256,
        transaction_index: u32,
        from_address: Address,
        to_address: Option<Address>,
        value: U256,
        nonce: u32,
        gas_limit: U256,
        gas_price: u128,
        data: Vec<u8>,
        r: [u8; 32],
        s: [u8; 32],
        v: u64,
        cumulative_gas_used: U256,
        gas_used: U256,
        logs_bloom: Vec<u8>,
        contract_address: Option<Address>,
        status: bool,
    ) -> Transaction {
        Transaction {
            gw_tx_hash,
            chain_id,
            block_number,
            block_hash,
            transaction_index,
            from_address,
            to_address,
            value,
            nonce,
            gas_limit,
            gas_price,
            data,
            v,
            r,
            s,
            cumulative_gas_used,
            gas_used,
            logs_bloom,
            contract_address,
            status,
        }
    }

    fn add_chain_replay_protection(&self) -> u64 {
        self.v as u64
            + if let Some(n) = self.chain_id {
                35 + n * 2
            } else {
                27
            }
    }

    pub fn to_rlp(&self) -> Vec<u8> {
        // RLP encode
        let mut s = rlp::RlpStream::new();
        s.begin_unbounded_list()
            .append(&self.nonce)
            .append(&self.gas_price)
            .append(&self.gas_limit);
        match self.to_address.as_ref() {
            Some(addr) => {
                s.append(&addr.to_vec());
            }
            None => {
                s.append(&vec![0u8; 0]);
            }
        };
        s.append(&self.value)
            .append(&self.data)
            .append(&self.add_chain_replay_protection())
            .append(&self.r.to_vec())
            .append(&self.s.to_vec());
        s.finalize_unbounded_list();
        s.out().freeze().to_vec()
    }

    pub fn compute_eth_tx_hash(&self) -> gw_common::H256 {
        // RLP encode
        let rlp_data = self.to_rlp();
        let mut hasher = Keccak256::new();
        hasher.update(&rlp_data);
        let buf = hasher.finalize();
        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(&buf);
        tx_hash.into()
    }
}

#[derive(Debug)]
pub struct Log {
    pub transaction_hash: H256,
    pub transaction_index: u32,
    pub block_number: u64,
    pub block_hash: H256,
    pub address: Address,
    pub data: Vec<u8>,
    pub log_index: u32,
    pub topics: Vec<H256>,
}

impl Log {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transaction_hash: H256,
        transaction_index: u32,
        block_number: u64,
        block_hash: H256,
        address: Address,
        data: Vec<u8>,
        log_index: u32,
        topics: Vec<H256>,
    ) -> Log {
        Log {
            transaction_hash,
            transaction_index,
            block_number,
            block_hash,
            address,
            data,
            log_index,
            topics,
        }
    }
}

#[derive(Debug)]
pub struct TransactionWithLogs {
    pub tx: Transaction,
    pub logs: Vec<Log>,
}
