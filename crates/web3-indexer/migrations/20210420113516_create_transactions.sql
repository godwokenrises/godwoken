-- Add migration script here
CREATE TABLE transactions (
    id BIGSERIAL PRIMARY KEY,
    hash TEXT UNIQUE NOT NULL,
    block_number NUMERIC REFERENCES blocks(number) NOT NULL,
    block_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    from_address TEXT NOT NULL,
    to_address TEXT,
    value NUMERIC NOT NULL,
    nonce NUMERIC,
    gas_limit NUMERIC,
    gas_price NUMERIC,
    input TEXT,
    v TEXT NOT NULL,
    r TEXT NOT NULL,
    s TEXT NOT NULL,
    cumulative_gas_used NUMERIC,
    gas_used NUMERIC,
    logs_bloom TEXT NOT NULL,
    contract_address TEXT,
    status BOOLEAN NOT NULL
);

CREATE INDEX ON transactions (block_number);
CREATE INDEX ON transactions (block_hash);
CREATE INDEX ON transactions (from_address);
CREATE INDEX ON transactions (to_address);
CREATE INDEX ON transactions (contract_address);
CREATE UNIQUE INDEX block_number_transaction_index_idx ON transactions (block_number, transaction_index);
CREATE UNIQUE INDEX block_hash_transaction_index_idx ON transactions (block_hash, transaction_index);