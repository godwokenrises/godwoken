-- Add migration script here
CREATE TABLE logs (
    id BIGSERIAL PRIMARY KEY,
    transaction_id BIGSERIAL REFERENCES transactions(id) NOT NULL,
    transaction_hash TEXT NOT NULL,
    transaction_index INTEGER NOT NULL,
    block_number NUMERIC REFERENCES blocks(number) NOT NULL,
    block_hash TEXT NOT NULL,
    address TEXT NOT NULL,
    data TEXT NOT NULL,
    log_index INTEGER NOT NULL,
    topics TEXT[] NOT NULL
);

CREATE INDEX ON logs (transaction_hash);
CREATE INDEX ON logs (block_hash);
CREATE INDEX ON logs (address)