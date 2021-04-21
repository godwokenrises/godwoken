use crate::{
    helper::PolyjuiceArgs,
    types::{Block as Web3Block, Log as Web3Log, Transaction as Web3Transaction},
};
use ckb_hash::blake2b_256;
use ckb_types::{
    packed::{self as ckb_packed, WitnessArgs},
    prelude::Unpack as CkbUnpack,
    H256,
};
use faster_hex;
use gw_common::state::State;
use gw_traits::CodeStore;
// use gw_chain::chain::Chain;
use gw_common::builtins::CKB_SUDT_ACCOUNT_ID;
// use gw_generator::backend_manage::SUDT_VALIDATOR_CODE_HASH;
// use gw_generator::traits::CodeStore;
use gw_store::{
    state_db::{StateDBTransaction, StateDBVersion},
    Store,
};
use gw_types::{
    packed::{
        BlockInfo, L2TransactionVec, RawL2Transaction, SUDTArgs, SUDTArgsUnion, SUDTQuery,
        SUDTTransfer, Script,
    },
    prelude::*,
};
use gw_types::{
    packed::{L2Block, Transaction},
    prelude::*,
};
use rust_decimal::Decimal;
use sqlx::types::chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::PgPool;

pub async fn insert_to_sql(
    pool: &PgPool,
    store: Store,
    l1_transaction: &Transaction,
    l2_sudt_type_script_hash: H256,
    polyjuice_type_script_hash: H256,
) -> anyhow::Result<()> {
    let l2_block = extract_l2_block(l1_transaction)?;
    let number: u64 = l2_block.raw().number().unpack();
    let row: Option<(Decimal,)> =
        sqlx::query_as("SELECT number FROM blocks ORDER BY number DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    println!(
        "The latest block number in database: {:?}, current syncing block number: {}",
        row, number
    );
    if row.is_none() || Decimal::from(number) == row.unwrap().0 + Decimal::from(1) {
        let web3_transactions = filter_web3_transactions(
            store.clone(),
            l2_block.clone(),
            l2_sudt_type_script_hash,
            polyjuice_type_script_hash,
        )
        .await?;
        let web3_block = build_web3_block(&pool, &l2_block, &web3_transactions).await?;
        // let web3_logs = build_web3_logs(&l2_block, &web3_transactions);
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO blocks (number, hash, parent_hash, logs_bloom, gas_limit, gas_used, timestamp, miner, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(web3_block.number)
            .bind(web3_block.hash)
            .bind(web3_block.parent_hash)
            .bind(web3_block.logs_bloom)
            .bind(web3_block.gas_limit)
            .bind(web3_block.gas_used)
            .bind(web3_block.timestamp)
            .bind(web3_block.miner)
            .bind(web3_block.size)
            .execute(&mut tx).await?;
        for web3_tx in web3_transactions {
            println!("web3_tx: {:?}", web3_tx);
            match sqlx::query("INSERT INTO transactions
            (hash, block_number, block_hash, transaction_index, from_address, to_address, value, nonce, gas_limit, gas_price, input, v, r, s, cumulative_gas_used, gas_used, logs_bloom, contract_address, status) 
            VALUES 
            ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)")
            .bind(web3_tx.hash)
            .bind(web3_tx.block_number)
            .bind(web3_tx.block_hash)
            .bind(web3_tx.transaction_index)
            .bind(web3_tx.from_address)
            .bind(web3_tx.to_address)
            .bind(web3_tx.value)
            .bind(web3_tx.nonce)
            .bind(web3_tx.gas_limit)
            .bind(web3_tx.gas_price)
            .bind(web3_tx.input)
            .bind(web3_tx.v)
            .bind(web3_tx.r)
            .bind(web3_tx.s)
            .bind(web3_tx.cumulative_gas_used)
            .bind(web3_tx.gas_used)
            .bind(web3_tx.logs_bloom)
            .bind(web3_tx.contract_address)
            .bind(web3_tx.status)
            .execute(&mut tx)
            .await {
                Ok(_s) => (),
                Err(e) => {
                    panic!("insert web3 transaction error: {:?}", e)
                },
            };
        }
        tx.commit().await.unwrap()
    }
    Ok(())
}

fn extract_l2_block(l1_transaction: &Transaction) -> anyhow::Result<L2Block> {
    let witness = l1_transaction
        .witnesses()
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("Witness missing for L2 block!"))?;
    let witness_args = WitnessArgs::from_slice(&witness.raw_data())?;
    let l2_block_bytes = witness_args
        .output_type()
        .to_opt()
        .ok_or_else(|| anyhow::anyhow!("Missing L2 block!"))?;
    let l2_block = L2Block::from_slice(&l2_block_bytes.raw_data())?;
    Ok(l2_block)
}

async fn filter_web3_transactions(
    store: Store,
    l2_block: L2Block,
    l2_sudt_type_script_hash: H256,
    polyjuice_type_script_hash: H256,
) -> anyhow::Result<Vec<Web3Transaction>> {
    let block_number = l2_block.raw().number().unpack();
    let block_hash: H256 = blake2b_256(l2_block.raw().as_slice()).into();
    // let chain = chain.read();
    let mut cumulative_gas_used = Decimal::from(0u32);
    let l2_transactions = l2_block.transactions();
    let mut web3_transactions: Vec<Web3Transaction> = vec![];
    let mut tx_index = 0i32;
    for l2_transaction in l2_transactions {
        // extract to_id corresponding script, check code_hash is either polyjuice contract code_hash or sudt contract code_hash
        let to_id = l2_transaction.raw().to_id().unpack();
        let to_script_hash = get_script_hash(store.clone(), to_id).await?;
        let to_script = match get_script(store.clone(), to_script_hash).await? {
            Some(s) => s,
            None => continue,
        };
        if to_script.code_hash().as_slice() == polyjuice_type_script_hash.0 {
            let tx_hash: H256 = blake2b_256(l2_transaction.raw().as_slice()).into();
            println!("tx_hash: {}", tx_hash);
            // extract from_id correspoding script, from_address is the script's args
            let from_id: u32 = l2_transaction.raw().from_id().unpack();
            let from_address = {
                let from_script_hash = get_script_hash(store.clone(), from_id).await?;
                let from_script = get_script(store.clone(), from_script_hash).await?.unwrap();
                from_script.args()
            };
            println!("Check from_address: {:#x}", from_address);
            let l2_tx_args = l2_transaction.raw().args();
            let polyjuice_args = PolyjuiceArgs::decode(l2_tx_args.raw_data().as_ref())?;
            // to_address is null if it's a contract deployment transaction
            let to_address = if polyjuice_args.is_create {
                None
            } else {
                let address = String::from("0x000000000000000000000000000000000000000f");
                Some(address)
            };
            println!("Check to_address: {:?}", to_address);
            let nonce = {
                let nonce: u32 = l2_transaction.raw().nonce().unpack();
                Decimal::from(nonce)
            };
            println!("Check nonce: {}", nonce);
            println!("input: {:?}", polyjuice_args.input);
            let input = match polyjuice_args.input {
                Some(input) => {
                    println!("input: {:?}", input);
                    let input_hex = faster_hex::hex_string(&input[..])?;
                    println!("input_hex: {}", input_hex);
                    Some(input_hex)
                }
                None => None,
            };

            let signature: [u8; 65] = l2_transaction.signature().unpack();
            let r = format!("0x{}", faster_hex::hex_string(&signature[0..31])?);
            let s = format!("0x{}", faster_hex::hex_string(&signature[32..63])?);
            let v = format!("0x{}", faster_hex::hex_string(&[signature[64]])?);
            let contract_address = if polyjuice_args.is_create {
                // TODO tx-receipt need return newly created account_id to contruct contract_address
                // let address = account_id_to_eth_address(id, false);
                let address = String::from("0x000000000000000000000000000000000000000f");
                Some(address)
            } else {
                None
            };
            println!("Check contract_address: {:?}", contract_address);
            let web3_transaction = Web3Transaction {
                hash: format!("{:#x}", tx_hash),
                transaction_index: tx_index as i32,
                block_number: Decimal::from(block_number),
                block_hash: format!("{:#x}", block_hash),
                from_address: format!("{:#x}", from_address),
                to_address: to_address,
                value: Decimal::from(polyjuice_args.value),
                nonce: nonce,
                gas_limit: Decimal::from(0),
                gas_price: Decimal::from(0),
                input: input,
                r: r,
                s: s,
                v: v,
                cumulative_gas_used: Decimal::from(0),
                gas_used: Decimal::from(0),
                logs_bloom: String::from("0x"),
                contract_address: contract_address,
                status: true,
            };

            println!("web3 transaction: {:?}", web3_transaction);
            web3_transactions.push(web3_transaction);
            tx_index += 1;
        } else if to_id == CKB_SUDT_ACCOUNT_ID
            && to_script.code_hash().as_slice() == l2_sudt_type_script_hash.0
        {
            // deal with CKB transfer
            let sudt_args =
                match SUDTArgs::from_slice(l2_transaction.raw().args().raw_data().as_ref()) {
                    Ok(s) => s,
                    Err(e) => {
                        println!("SUDArgs error: {:?}", e);
                        continue;
                    }
                };
            match sudt_args.to_enum() {
                SUDTArgsUnion::SUDTTransfer(sudt_transfer) => {
                    let to_id: u32 = sudt_transfer.to().unpack();
                    let amount: u128 = sudt_transfer.amount().unpack();
                    let fee: u128 = sudt_transfer.fee().unpack();
                    let tx_hash: H256 = blake2b_256(l2_transaction.raw().as_slice()).into();

                    let to_script_hash = get_script_hash(store.clone(), to_id).await?;
                    let to_script = match get_script(store.clone(), to_script_hash).await? {
                        Some(s) => s,
                        None => continue,
                    };

                    let from_id = l2_transaction.raw().from_id().unpack();
                    let from_address = {
                        let from_script_hash = get_script_hash(store.clone(), from_id).await?;
                        let from_script =
                            get_script(store.clone(), from_script_hash).await?.unwrap();
                        from_script.args()
                    };

                    let to_address = format!("{:#x}", to_script.args());
                    let value = amount;
                    let gas_limit = Decimal::from(0);
                    let gas_price = Decimal::from(0);

                    let nonce = {
                        let nonce: u32 = l2_transaction.raw().nonce().unpack();
                        Decimal::from(nonce)
                    };

                    let input = "0x".to_owned();

                    let signature: [u8; 65] = l2_transaction.signature().unpack();
                    let r = format!("0x{}", faster_hex::hex_string(&signature[0..31])?);
                    let s = format!("0x{}", faster_hex::hex_string(&signature[32..63])?);
                    let v = format!("0x{}", faster_hex::hex_string(&[signature[64]])?);

                    let web3_transaction = Web3Transaction {
                        hash: format!("{:#x}", tx_hash),
                        transaction_index: tx_index as i32,
                        block_number: Decimal::from(block_number),
                        block_hash: format!("{:#x}", block_hash),
                        from_address: format!("{:#x}", from_address),
                        to_address: Some(to_address),
                        value: Decimal::from(value),
                        nonce: nonce,
                        gas_limit: gas_limit,
                        gas_price: gas_price,
                        input: Some(input),
                        r: r,
                        s: s,
                        v: v,
                        cumulative_gas_used: Decimal::from(0),
                        gas_used: Decimal::from(0),
                        logs_bloom: String::from("0x"),
                        contract_address: None,
                        status: true,
                    };

                    println!("web3 transaction: {:?}", web3_transaction);
                    web3_transactions.push(web3_transaction);
                }
                SUDTArgsUnion::SUDTQuery(sudt_query) => {}
            }
            tx_index += 1;
        }
    }
    Ok(web3_transactions)
}

async fn build_web3_block(
    pool: &PgPool,
    l2_block: &L2Block,
    web3_transactions: &Vec<Web3Transaction>,
) -> anyhow::Result<Web3Block> {
    let block_number = l2_block.raw().number().unpack();
    let block_hash: H256 = blake2b_256(l2_block.raw().as_slice()).into();
    let parent_hash = {
        if block_number == 0 {
            String::from("0x0000000000000000000000000000000000000000000000000000000000000000")
        } else {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT hash FROM blocks WHERE number = $1")
                    .bind(Decimal::from(block_number - 1))
                    .fetch_optional(pool)
                    .await?;
            match row {
                Some(block) => block.0,
                None => panic!("No parent hash found!"),
            }
        }
    };
    let epoch_time: u64 = l2_block.raw().timestamp().unpack();
    let web3_block = Web3Block {
        number: Decimal::from(block_number),
        hash: format!("{:#x}", block_hash),
        // TODO update parent_hash
        parent_hash: parent_hash,
        logs_bloom: String::from(""),
        gas_limit: Decimal::from(0),
        // gas_used: last_web3_tx.cumulative_gas_used,
        gas_used: Decimal::from(0),
        miner: format!("{}", l2_block.raw().block_producer_id()),
        size: Decimal::from(0),
        timestamp: DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp(epoch_time as i64, 0),
            Utc,
        ),
    };
    Ok(web3_block)
}

// fn build_web3_logs(
//     l2_block: &L2Block,
//     web3_transactions: &Vec<Web3Transaction>,
// ) -> anyhow::Result<Vec<Web3Log>> {
// }

// async fn insert_to_block(tx: & mut, block: Web3Block) -> anyhow::Result<()> {
//     sqlx::query("INSERT INTO blocks (number, hash, parent_hash, logs_bloom, gas_limit, gas_used, timestamp, miner, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
//      .bind(block.number)
//      .bind(block.hash)
//      .bind(block.parent_hash)
//      .bind(block.logs_bloom)
//      .bind(block.gas_limit)
//      .bind(block.gas_used)
//      .bind(block.timestamp)
//      .bind(block.miner)
//      .bind(block.size)
//     //  .bind(format!("{}", l2_block.raw().aggregator_id()))
//     //  .bind(l2_block.as_slice().len() as i64)
//      .execute(tx).await?;
//     Ok(())
// }

fn insert_to_transaction(tx: Web3Transaction) {
    // sqlx::query("INSERT INTO transactions (hash, block_number, block_hash, transaction_index, from_address, to_address, value, nonce, gas_limit, gas_price, input, v, r, s, cumulative_gas_used, gas_used, logs_bloom, contract_address, status) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)")
    // .bind(format!("{:#x}", tx_hash))
    // .bind(number as i64)
    // .bind(format!("{:#x}", hash))
    // .bind(tx_index as i32)
    // .bind(format!("{:#x}", from_address))
    // .bind(to_address)
    // .bind(value)
    // .bind(nonce)
    // .bind(gas_limit)
    // .bind(gas_price)
    // .bind(format!("{:#x}", input))
    // .bind(format!("{:#x}", r))
    // .bind(format!("{:#x}", s))
    // .bind(format!("{:#x}", v))
    // // .bind(cumulative_gas_used)
    // // .bind(gas_used)
    // //.bind(logs_bloom)
    // .bind(contract_address)
    // // .bind(status)
    // .execute(&mut tx).await?;
}

fn insert_to_log(log: Web3Log) {}

async fn get_script_hash(store: Store, account_id: u32) -> anyhow::Result<gw_common::H256> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db =
        StateDBTransaction::from_version(&db, StateDBVersion::from_block_hash(tip_hash))?;
    let tree = state_db.account_state_tree()?;

    let script_hash = tree.get_script_hash(account_id)?;
    Ok(script_hash)
}

async fn get_script(store: Store, script_hash: gw_common::H256) -> anyhow::Result<Option<Script>> {
    let db = store.begin_transaction();
    let tip_hash = db.get_tip_block_hash()?;
    let state_db =
        StateDBTransaction::from_version(&db, StateDBVersion::from_block_hash(tip_hash))?;
    let tree = state_db.account_state_tree()?;

    let script_opt = tree.get_script(&script_hash);
    Ok(script_opt)
}
