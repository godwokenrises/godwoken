use gw_types::h256::*;
use gw_types::{offchain::RunResult, U256};
use lib::{
    ctx::MockChain,
    helper::{parse_log, Log},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryInto,
    fs, io,
    path::{Path, PathBuf},
    u128,
};

const TEST_CASE_DIR: &str = "../integration-test/ethereum-tests/GeneralStateTests/VMTests/";
const HARD_FORKS: &[&str] = &["Berlin", "Istanbul"];
const EXCLUDE_TEST_FILES: &[&str] = &["loopMul.json", "loopExp.json"];

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Info {
    comment: String,
    #[serde(rename = "filling-rpc-server")]
    filling_rpc_server: String,
    #[serde(rename = "filling-tool-version")]
    filling_tool_version: String,
    #[serde(rename = "generatedTestHash")]
    generated_test_hash: String,
    labels: Option<HashMap<String, String>>,
    lllcversion: String,
    solidity: String,
    source: String,
    #[serde(rename = "sourceHash")]
    source_hash: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Env {
    current_base_fee: String,
    current_coinbase: String,
    current_difficulty: String,
    current_gas_limit: String,
    current_number: String,
    current_timestamp: String,
    previous_hash: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Pre {
    balance: String,
    code: String,
    nonce: String,
    storage: HashMap<String, String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Post {
    hash: String,
    indexes: Indexes,
    logs: String,
    #[serde(rename = "txbytes")]
    tx_bytes: String,
}

#[derive(Deserialize, Debug)]
struct Indexes {
    data: usize,
    gas: usize,
    value: usize,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Transaction {
    data: Vec<String>,
    #[serde(rename = "gasLimit")]
    gas_limit: Vec<String>,
    #[serde(rename = "gasPrice")]
    gas_price: String,
    nonce: String,
    sender: String,
    to: String,
    value: Vec<String>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct TestCase {
    #[serde(rename = "_info")]
    info: Info,
    env: Env,
    pre: BTreeMap<String, Pre>,
    post: BTreeMap<String, Vec<Post>>,
    transaction: Transaction,
}

struct VMTestRunner {
    testcase: TestCase,
}

impl VMTestRunner {
    fn new(testcase: TestCase) -> anyhow::Result<Self> {
        Ok(Self { testcase })
    }

    // handle pre
    // reset chain
    // create accounts and fill with balance, code, storage
    fn init(&self) -> anyhow::Result<MockChain> {
        //reset chain for each test
        let mut chain = MockChain::setup("..")?;

        for (eth_addr, account) in self.testcase.pre.iter() {
            println!("init account for: {}", &eth_addr);
            let balance = U256::from_str_radix(&account.balance, 16)?;

            let eth_addr = hex::decode(eth_addr.trim_start_matches("0x"))?;
            let eth_addr: [u8; 20] = eth_addr.try_into().unwrap();

            if account.code != "0x" {
                let code = hex::decode(&account.code.trim_start_matches("0x"))?;
                let mut storage = HashMap::with_capacity(account.storage.len());
                for (k, v) in &account.storage {
                    let k = hex_to_h256(k)?;
                    let v = hex_to_h256(v)?;
                    storage.insert(k, v);
                }
                let account_id =
                    chain.create_contract_account(&eth_addr, balance, &code, storage)?;
                println!("Contract account id {} created", account_id);
            } else {
                let account_id = chain.create_eoa_account(&eth_addr, balance)?;
                println!("EOA account id {} created", account_id);
            }
        }
        Ok(chain)
    }

    fn run(&self) -> anyhow::Result<()> {
        // prepare tx form `post`
        for hardfork in HARD_FORKS {
            // init ctx for each `post`
            let mut chain = self.init()?;
            if let Some(posts) = self.testcase.post.get(&hardfork.to_string()) {
                println!("Prepare tx, hardfork: {}", hardfork);
                for post in posts {
                    self.run_tx(post, &mut chain)?;
                }
            }
        }
        Ok(())
    }

    fn run_tx(&self, post: &Post, chain: &mut MockChain) -> anyhow::Result<()> {
        let transaction = &self.testcase.transaction;
        let gas = transaction
            .gas_limit
            .get(post.indexes.gas)
            .expect("gas limit");
        let gas_limit = U256::from_str_radix(gas, 16)?;
        let data = transaction.data.get(post.indexes.data).expect("data");
        let data = hex::decode(data.trim_start_matches("0x"))?;
        let value = transaction.value.get(post.indexes.value).expect("value");
        let value = U256::from_str_radix(value, 16)?;

        let gas_price = &transaction.gas_price;
        let gas_price = U256::from_str_radix(gas_price, 16)?;
        let from_eth_addr = hex_to_eth_address(&transaction.sender)?;
        let to_eth_addr = hex_to_eth_address(&transaction.to)?;
        let from_id = chain
            .get_account_id_by_eth_address(&from_eth_addr)?
            .expect("from_id");
        let to_id = chain
            .get_account_id_by_eth_address(&to_eth_addr)?
            .expect("to_id");
        let tx = Tx {
            from_id,
            to_id,
            code: &data,
            gas_limit: gas_limit.as_u64(),
            gas_price: gas_price.as_u128(),
            value: value.as_u128(),
        };
        let sub_test_case = SubTestCase { chain, tx };
        let run_result = sub_test_case.run()?;
        let logs_hash = rlp_log_hash(&run_result);
        let expect_logs_hash = hex::decode(post.logs.trim_start_matches("0x"))?;
        assert_eq!(logs_hash.as_slice(), &expect_logs_hash);
        Ok(())
    }
}

struct Tx<'a> {
    from_id: u32,
    to_id: u32,
    code: &'a [u8],
    gas_limit: u64,
    gas_price: u128,
    value: u128,
}

struct SubTestCase<'a, 'b> {
    chain: &'a mut MockChain,
    tx: Tx<'b>,
}

impl<'a, 'b> SubTestCase<'a, 'b> {
    fn run(self) -> anyhow::Result<RunResult> {
        let Tx {
            from_id,
            to_id,
            code,
            gas_limit,
            gas_price,
            value,
        } = self.tx;
        let run_result = self
            .chain
            .execute(from_id, to_id, code, gas_limit, gas_price, value)?;
        if run_result.exit_code != 0 {
            return Err(anyhow::anyhow!("Test case failed."));
        }
        Ok(run_result)
    }
}

fn rlp_log_hash(run_result: &RunResult) -> H256 {
    let mut stream = rlp::RlpStream::new();
    stream.begin_unbounded_list();
    run_result.logs.iter().for_each(|l| {
        let log = parse_log(l);
        if let Log::PolyjuiceUser {
            address,
            data,
            topics,
        } = log
        {
            stream.begin_list(3);
            stream.append(&address.to_vec());
            stream.begin_list(topics.len());
            topics.iter().for_each(|t| {
                stream.append(&t.as_slice());
            });
            if data.is_empty() {
                stream.append_empty_data();
            } else {
                stream.append(&data);
            }
        }
    });
    stream.finalize_unbounded_list();
    let log_hash = tiny_keccak::keccak256(&stream.out().freeze());
    log_hash.into()
}

fn hex_to_h256(hex_str: &str) -> anyhow::Result<H256> {
    const PREFIX: &str = "0x";
    let hex_str = if hex_str.starts_with(PREFIX) {
        hex_str.trim_start_matches("0x")
    } else {
        hex_str
    };
    let buf = hex::decode(hex_str)?;
    assert!(buf.len() <= 32);
    let mut key = [0u8; 32];
    if buf.len() < 32 {
        let idx = 32 - buf.len();
        key[idx..].copy_from_slice(&buf);
    } else {
        key.copy_from_slice(&buf);
    };
    let key = H256::from(key);

    Ok(key)
}

fn hex_to_eth_address(hex_str: &str) -> anyhow::Result<[u8; 20]> {
    const PREFIX: &str = "0x";
    let hex_str = if hex_str.starts_with(PREFIX) {
        hex_str.trim_start_matches("0x")
    } else {
        hex_str
    };
    let buf = hex::decode(hex_str)?;
    assert_eq!(buf.len(), 20);
    let eth_address = buf.try_into().unwrap();
    Ok(eth_address)
}

fn read_all_files(path: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for file in fs::read_dir(path)? {
        let p = file?.path();
        if p.is_dir() {
            read_all_files(p.as_path(), paths)?;
        } else {
            paths.push(p);
        }
    }

    Ok(())
}
#[test]
fn ethereum_test() -> anyhow::Result<()> {
    let mut paths = Vec::new();
    read_all_files(Path::new(TEST_CASE_DIR), &mut paths)?;
    let mut err_cases = Vec::new();
    for path in paths {
        // Skip testcases in `EXCLUDE_TEST_FILES`.
        if let Some(filename) = path.file_name() {
            if let Some(filename) = filename.to_str() {
                if EXCLUDE_TEST_FILES.contains(&filename) {
                    continue;
                }
            }
        }
        println!("Starting test with: {:?}", &path);
        let content = fs::read_to_string(&path)?;
        let test_cases: HashMap<String, TestCase> = serde_json::from_str(&content)?;
        for (testname, testcase) in test_cases {
            println!("test name: {}", testname);
            let runner = VMTestRunner::new(testcase)?;
            if runner.run().is_err() {
                err_cases.push(path.clone());
            }
        }
    }
    if !err_cases.is_empty() {
        println!("============================================================================");
        println!("============================Error test case paths===========================");
        for path in err_cases {
            println!("{:?}", &path);
        }
        println!("============================================================================");
        println!("============================================================================");
        return Err(anyhow::anyhow!("Some tests are failed."));
    }
    Ok(())
}

#[test]
fn ethereum_failure_test() -> anyhow::Result<()> {
    let mut paths = Vec::new();
    read_all_files(Path::new(TEST_CASE_DIR), &mut paths)?;
    let mut err_cases = Vec::new();
    for path in paths {
        if let Some(filename) = path.file_name() {
            if let Some(filename) = filename.to_str() {
                if EXCLUDE_TEST_FILES.contains(&filename) {
                    println!("Starting test with: {:?}", &path);
                    let content = fs::read_to_string(&path)?;
                    let test_cases: HashMap<String, TestCase> = serde_json::from_str(&content)?;
                    for (testname, testcase) in test_cases {
                        println!("test name: {}", testname);
                        let runner = VMTestRunner::new(testcase)?;
                        if runner.run().is_err() {
                            err_cases.push(path.clone());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
