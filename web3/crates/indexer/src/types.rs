use gw_types::h256::H256;
use gw_types::U256;
use sha3::{Digest, Keccak256};
use sqlx::types::chrono::{DateTime, Utc};

type Address = [u8; 20];

#[derive(Debug)]
pub struct Block {
    pub number: u64,
    pub hash: H256,
    pub parent_hash: H256,
    pub gas_limit: u128,
    pub gas_used: u128,
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
    pub gas_limit: u128,
    pub gas_price: u128,
    pub data: Vec<u8>,
    pub v: u8,
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub cumulative_gas_used: u128,
    pub gas_used: u128,
    pub contract_address: Option<Address>,
    pub exit_code: u8,
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
        gas_limit: u128,
        gas_price: u128,
        data: Vec<u8>,
        r: [u8; 32],
        s: [u8; 32],
        v: u8,
        cumulative_gas_used: u128,
        gas_used: u128,
        contract_address: Option<Address>,
        exit_code: u8,
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
            contract_address,
            exit_code,
        }
    }

    fn add_chain_replay_protection(&self) -> u64 {
        self.v as u64
            + if let Some(id) = self.chain_id {
                // For non eip-155 txs
                if id == 0 {
                    27
                } else {
                    35 + id * 2
                }
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
        // r & s should be integer format in RLP
        let r_num = U256::from(&self.r);
        let s_num = U256::from(&self.s);
        s.append(&self.value)
            .append(&self.data)
            .append(&self.add_chain_replay_protection())
            .append(&r_num)
            .append(&s_num);
        s.finalize_unbounded_list();
        s.out().freeze().to_vec()
    }

    pub fn compute_eth_tx_hash(&self) -> gw_types::h256::H256 {
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
