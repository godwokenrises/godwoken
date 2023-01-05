# Schema Design

## Sql Schema(postgresql)
```sql
CREATE TABLE blocks (
    number numeric PRIMARY KEY,
    hash bytea NOT NULL,
    parent_hash bytea NOT NULL,
    gas_limit numeric NOT NULL,
    gas_used numeric NOT NULL,
    miner bytea NOT NULL,
    size integer NOT NULL,
    "timestamp" timestamp with time zone NOT NULL
);

create unique index on blocks(hash);

CREATE TABLE transactions (
    id BIGSERIAL PRIMARY KEY,
    hash bytea NOT NULL,
    eth_tx_hash bytea NOT NULL,
    block_number numeric REFERENCES blocks(number) NOT NULL,,
    block_hash bytea NOT NULL,
    transaction_index integer NOT NULL,
    from_address bytea NOT NULL,
    to_address bytea,
    value numeric(80,0) NOT NULL,
    nonce bigint NOT NULL,
    gas_limit numeric,
    gas_price numeric,
    input bytea,
    v smallint NOT NULL,
    r bytea NOT NULL,
    s bytea NOT NULL,
    cumulative_gas_used numeric,
    gas_used numeric,
    contract_address bytea,
    exit_code smallint NOT NULL
);

CREATE INDEX ON transactions (from_address);
CREATE INDEX ON transactions (to_address);
CREATE INDEX ON transactions (contract_address);
CREATE UNIQUE INDEX block_number_transaction_index_idx ON transactions (block_number, transaction_index);
CREATE UNIQUE INDEX block_hash_transaction_index_idx ON transactions (block_hash, transaction_index);

CREATE TABLE logs (
    id BIGSERIAL PRIMARY KEY,
    transaction_id bigint REFERENCES transactions(id) NOT NULL,
    transaction_hash bytea NOT NULL,
    transaction_index integer NOT NULL,
    block_number numeric REFERENCES blocks(number) NOT NULL,
    block_hash bytea NOT NULL,
    address bytea NOT NULL,
    data bytea,
    log_index integer NOT NULL,
    topics bytea[] NOT NULL
);

CREATE INDEX ON logs (transaction_id);
CREATE INDEX ON logs (transaction_hash);
CREATE INDEX ON logs (block_hash);
CREATE INDEX ON logs (address);
CREATE INDEX ON logs (block_number);
```

## 字段含义

### block

- number: 区块高度，由于godwoken只会revert不会分叉，在同一个高度只存在一个区块，故可以作为主键
- hash: 区块哈希
- parent_hash: 上一个区块hash
- gas_limit: 该区块最多能花的gas_limit, 等于各个交易的gas_limit之和,这个值在eth里面现在不超过12.5million
- gas_used: 该区块内所有交易花费的gas之和
- timestamp: 区块的时间戳
- miner: godwoken里指的是block producer，这里miner字段与web3接口保持一致
- size: 区块大小，bytes


### transaction
- hash: 交易哈希
- eth_tx_hash: 交易的以太坊格式交易哈希
- block_number：区块高度
- block_hash：区块哈希
- transaction_index：交易在区块里的位置，这个和L2Transaction在L2Block的位置存在差异
- from_address：交易发出方，对应godwoken里面L2Transaction的from_id
- to_address: 交易接受方，在eth中如果是合约创建交易则为null；在godwoken中需要解析L2Transaction的args（不同于to_id概念），提取出sudt转账交易的接受账户，或者是polyjuice交易的接受账户(合约)
- value: 转账额度(是sudt的转账还是polyjuice的转账？)
- nonce: 地址发出过的交易数量，单调递增（polyjuice交易是否有单独的nonce?)
- gas_limit: polyjuice交易的gas_limit，非polyjuice交易设置为0
- gas_price: polyjuice交易的gas_price，非polyjuice交易设置为0
- input: solidity合约调用的input，非polyjuice交易设置为null
- v: ECDSA recovery ID
- r: ECDSA signature
- s: ECDSA signature
- cumulative_gas_used: 该区块里当前交易和之前的交易花费的gas之和
- gas_used：交易实际花费的gas
- log_bloom：该交易中logs的bloom filter
- contract_address: 如果是合约创建交易，这个则为创建的合约的地址；否则为null
- exit_code: 表示交易是否成功，0成功，其它为失败

### log
- transaction_id: 交易id，transaction表主键
- transaction_hash：交易哈希
- transaction_index：交易在区块中位置
- block_number：区块高度
- block_hash：区块哈希
- address：产生这条log的地址，一般是某个合约地址
- log_index：log在交易receipt中的位置
- topics：
  - topic[0]: Event的签名，`keccak(EVENT_NAME+"("+EVENT_ARGS.map(canonical_type_of).join(",")+")")` ，对于anonymous event不生成该topic
  - topic[1] ~ topic[3]: 被indexed字段修饰的Event参数
- data：non-indexed的Event参数
