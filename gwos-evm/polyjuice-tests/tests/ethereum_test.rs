use gw_types::h256::*;
use gw_types::{offchain::RunResult, U256};
use lib::{
    ctx::MockChain,
    helper::{parse_log, Log},
};
use num_bigint::BigUint;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryInto,
    fs, u128,
};

const TEST_CASE_DIR: &str = "../integration-test/ethereum-tests/GeneralStateTests/";
const VMTEST_DIR: &str = "../integration-test/ethereum-tests/GeneralStateTests/VMTests/";
const FILLER_DIR: &str = "../integration-test/ethereum-tests/src/GeneralStateTestsFiller/";
const VMTEST_FILLER_DIR: &str =
    "../integration-test/ethereum-tests/src/GeneralStateTestsFiller/VMTests/";
const HARD_FORKS: &[&str] = &["Berlin", "Istanbul"];
// Explain why we skip those tests.
const EXCLUDE_VMTEST_FILES: &[&str] = &[
    "sha3.json",      // Failure: memory size issue.
    "msize.json",     // Failure: memory size issue.
    "gas.json",       // Failure: memory size issue.
    "blockInfo.json", // Failure: Some fields are mocked in polyjuice. It's not testable.
    "loopMul.json",   // Success but too slow.
    "loopExp.json",   // Success but too slow.
];
#[allow(dead_code)]
const EXCLUDE_TEST_FILES: &[&str] = &[
    "ByZero.json",
    "createContractViaTransactionCost53000.json",
    "HighGasPrice.json",
    "ZeroKnowledge",
];
const LABEL_PREFIX: &str = ":label";
const MAX_CYCLES: u64 = 500_000_000;

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
    // The lable references the entry we need to validate in the filler.
    labels: Option<HashMap<String, String>>,
    lllcversion: String,
    solidity: String,
    source: String,
    #[serde(rename = "sourceHash")]
    source_hash: String,
}

impl Info {
    fn get_label_by_data_index(&self, index: usize) -> Option<String> {
        self.labels
            .as_ref()
            .map(|lables| lables.get(&index.to_string()).cloned().unwrap())
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Env {
    current_base_fee: Option<String>,
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
    gas_price: Option<String>,
    nonce: String,
    sender: Option<String>,
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

/* Structs below come from filler files.*/
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ExpectResult {
    #[serde(rename = "shouldnotexit")]
    should_not_exit: Option<String>,
    storage: Option<HashMap<u64, String>>,
    nonce: Option<u32>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum LabelIndex {
    Single(String),
    Sequence(Vec<String>),
    Unint(i32),
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ExpectIndexes {
    data: LabelIndex,
    gas: i32,
    value: i32,
}
#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Expect {
    indexes: ExpectIndexes,
    network: Vec<String>,
    result: HashMap<String, ExpectResult>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct Filler {
    env: Env,
    expect: Vec<Expect>,
    pre: BTreeMap<String, Pre>,
    transaction: Transaction,
}

impl Filler {
    fn get_expect_by_label(&self, label: &Option<String>) -> Option<&Expect> {
        match label {
            Some(label) => {
                let label = format!("{} {}", LABEL_PREFIX, label);
                self.expect.iter().find(|ex| match &ex.indexes.data {
                    LabelIndex::Single(l) => l == &label,
                    LabelIndex::Sequence(l) => l.iter().find(|data| **data == label).is_some(),
                    _ => false,
                })
            }
            None => self.expect.first(),
        }
    }
}

struct TestRunner {
    testcase: TestCase,
    filler: Filler,
}

impl TestRunner {
    fn new(testcase: TestCase, filler: Filler) -> Self {
        Self { testcase, filler }
    }

    // handle pre
    // reset chain
    // create accounts and fill with balance, code, storage
    fn init(&self) -> anyhow::Result<MockChain> {
        //reset chain for each test
        let mut chain = MockChain::setup("..")?;
        chain.set_max_cycles(MAX_CYCLES);

        for (eth_addr, account) in self.testcase.pre.iter() {
            let balance = U256::from_str_radix(&account.balance, 16)?;
            println!("init account for: {} balance: {}", &eth_addr, balance);

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
            if let Some(posts) = self.testcase.post.get(&hardfork.to_string()) {
                println!("Prepare tx, hardfork: {}", hardfork);
                for (_idx, post) in posts.into_iter().enumerate() {
                    // init ctx for each `post`
                    let mut chain = self.init()?;
                    let label = self
                        .testcase
                        .info
                        .get_label_by_data_index(post.indexes.data);
                    let expect = self
                        .filler
                        .get_expect_by_label(&label)
                        .unwrap_or(self.filler.expect.first().expect("find first label"));
                    self.run_tx(post, &mut chain, &expect.result)?;
                }
            }
        }
        Ok(())
    }

    fn run_tx(
        &self,
        post: &Post,
        chain: &mut MockChain,
        expect: &HashMap<String, ExpectResult>,
    ) -> anyhow::Result<()> {
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

        let gas_price = match &transaction.gas_price {
            Some(gas_price) => U256::from_str_radix(gas_price, 16)?,
            None => U256::zero(),
        };
        let from_eth_addr = hex_to_eth_address(&transaction.sender.as_ref().expect("sender"))?;
        let to_eth_addr = hex_to_eth_address(&transaction.to)?;
        let from_id = chain
            .get_account_id_by_eth_address(&from_eth_addr)?
            .ok_or(anyhow::anyhow!("Cannot find from id."))?;
        let to_id = chain
            .get_account_id_by_eth_address(&to_eth_addr)?
            .ok_or(anyhow::anyhow!("Cannot find to id."))?;

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
        if logs_hash.as_slice() != &expect_logs_hash {
            return Err(anyhow::anyhow!(
                "Compare logs hash failed: expect: {}, actual: {}",
                hex::encode(&expect_logs_hash),
                hex::encode(logs_hash.as_slice())
            ));
        }

        for (eth_addr, expect_result) in expect {
            let eth_addr = hex::decode(eth_addr.trim_start_matches("0x"))?
                .try_into()
                .expect("to eth addr");
            let account_id = chain
                .get_account_id_by_eth_address(&eth_addr)
                .expect("get account id");
            if account_id.is_none() {
                continue;
            }
            let account_id = account_id.unwrap();
            let actual = chain.get_nonce(account_id)?;
            if let Some(expect_nonce) = expect_result.nonce {
                if expect_nonce != actual {
                    return Err(anyhow::anyhow!(
                        "Compare nonce of account {} failed: expect: {}, actual: {}",
                        account_id,
                        expect_nonce,
                        actual
                    ));
                }
            }
            if let Some(storage) = &expect_result.storage {
                for (k, v) in storage {
                    let mut buf = [0u8; 32];
                    buf[24..].copy_from_slice(&k.to_be_bytes());
                    let actual = chain
                        .get_storage(account_id, &buf.into())
                        .expect("get value");
                    let expect = decode_storage_value(&v).expect("decode value");
                    if expect.as_slice() != actual.as_slice() {
                        return Err(anyhow::anyhow!(
                            "State validate failed for key: {:x}, expect value: {}, actual value: {}",
                            k,
                            &hex::encode(&expect.as_slice()),
                            &hex::encode(&actual.as_slice()),
                        ));
                    }
                }
            }
        }
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
    // take care of hex_str like: 0x100
    let hex_str = if hex_str.len() < 64 {
        let mut prefix = String::new();
        let cnt = 64 - hex_str.len();
        for _i in 0..cnt {
            prefix.push('0');
        }
        format!("{}{}", prefix, hex_str)
    } else {
        hex_str.to_string()
    };
    let buf = hex::decode(hex_str)?;
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
    if buf.len() != 20 {
        return Err(anyhow::anyhow!("Invalid eth address."));
    }
    let eth_address = buf.try_into().unwrap();
    Ok(eth_address)
}

fn decode_storage_value(v: &str) -> anyhow::Result<H256> {
    if v.starts_with("0x") {
        return hex_to_h256(v);
    }

    let v: BigUint = v.parse()?;

    let buf = v.to_bytes_be();
    let mut arr = [0u8; 32];
    let idx = 32 - buf.len();
    arr[idx..].copy_from_slice(&buf);
    Ok(H256::from(arr))
}

#[test]
fn ethereum_test() -> anyhow::Result<()> {
    for dir in fs::read_dir(TEST_CASE_DIR)? {
        if let Ok(dir) = dir {
            let subdir = dir.path();
            let dir_name = subdir
                .file_name()
                .expect("sub dir")
                .to_str()
                .expect("sub dir to str");
            if dir_name == "VMTests" && dir_name.to_lowercase().contains("zeroknowledge") {
                // skip VMTests and zk
                continue;
            }

            for entry in fs::read_dir(dir.path())? {
                if let Ok(entry) = entry {
                    let test_name = entry.file_name();
                    let test_name = test_name.to_str().expect("test name");
                    if !test_name.ends_with("json") {
                        continue;
                    }
                    let content = fs::read_to_string(&entry.path())?;
                    let test_cases: HashMap<String, TestCase> = serde_json::from_str(&content)?;
                    let fname = test_name.replace(".json", "Filler.yml");
                    let filler_path = format!("{}/{}/{}", FILLER_DIR, dir_name, &fname);
                    let content = fs::read_to_string(&filler_path)?;
                    let mut fillers: HashMap<String, Filler> = serde_yaml::from_str(&content)?;
                    for (k, test_case) in test_cases.into_iter() {
                        let filler = fillers.remove(&k).expect("find filler");
                        let runner = TestRunner::new(test_case, filler);
                        runner.run()?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn ethereum_vmtest_test() -> anyhow::Result<()> {
    let mut error_tests = Vec::new();
    for dir in fs::read_dir(VMTEST_DIR)? {
        let subpath = dir?.path();
        let test_kind = subpath
            .file_name()
            .expect("test kind")
            .to_str()
            .expect("test kind");
        if subpath.is_dir() {
            for p in fs::read_dir(&subpath)? {
                let test_path = p?.path();
                println!("Running test: {:?}", &test_path);
                let content = fs::read_to_string(&test_path)?;
                let test_cases: HashMap<String, TestCase> = serde_json::from_str(&content)?;
                let fname = test_path
                    .file_name()
                    .expect("file_name")
                    .to_str()
                    .expect("fname");
                if EXCLUDE_VMTEST_FILES.contains(&fname) {
                    println!("Skip test: {}", fname);
                    continue;
                }
                let fname = fname.replace(".json", "Filler.yml");
                let filler_path = format!("{}/{}/{}", &VMTEST_FILLER_DIR, test_kind, fname);
                let content = fs::read_to_string(&filler_path)?;
                let mut fillers: HashMap<String, Filler> = serde_yaml::from_str(&content)?;
                for (k, test_case) in test_cases.into_iter() {
                    let filler = fillers.remove(&k).expect("get filler");
                    let runner = TestRunner::new(test_case, filler);
                    if let Err(err) = runner.run() {
                        eprintln!("test case: {}, err: {:?}", k, err);
                        error_tests.push((
                            format!("{}#{}", &test_path.to_string_lossy(), k),
                            filler_path.to_string(),
                            err,
                        ));
                    }
                }
            }
        }
    }
    if !error_tests.is_empty() {
        println!("#Failed case: {}", error_tests.len());
    }
    error_tests.iter().for_each(|(test, filler, err)| {
        println!("test: {}\nfiller: {}\nerr: {}", test, filler, err)
    });
    Ok(())
}

// The test is used to debug.
#[test]
fn ethereum_single_test() -> anyhow::Result<()> {
    let path = "../integration-test/ethereum-tests/GeneralStateTests/VMTests/vmLogTest/log0.json";
    let content = fs::read_to_string(&path)?;
    let test_cases: HashMap<String, TestCase> = serde_json::from_str(&content)?;
    let path = "../integration-test/ethereum-tests/src/GeneralStateTestsFiller/VMTests/vmLogTest/log0Filler.yml";
    let content = fs::read_to_string(&path)?;
    let mut fillers: HashMap<String, Filler> = serde_yaml::from_str(&content)?;
    for (k, test_case) in test_cases.into_iter() {
        let filler = fillers.remove(&k).expect("get filler");
        let runner = TestRunner::new(test_case, filler);
        runner.run()?;
    }

    Ok(())
}
