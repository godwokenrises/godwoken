-- Add migration script here
CREATE TABLE blocks (
    number NUMERIC PRIMARY KEY,
    hash TEXT UNIQUE NOT NULL,
    parent_hash TEXT NOT NULL,
    logs_bloom TEXT NOT NULL,
    gas_limit NUMERIC NOT NULL,
    gas_used NUMERIC NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    miner TEXT NOT NULL,
    size NUMERIC NOT NULL
);
