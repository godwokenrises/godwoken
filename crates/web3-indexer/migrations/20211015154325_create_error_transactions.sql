-- Add migration script here
CREATE TABLE error_transactions (
    id BIGSERIAL PRIMARY KEY,
    hash TEXT UNIQUE NOT NULL,
    block_number NUMERIC NOT NULL,
    cumulative_gas_used NUMERIC,
    gas_used NUMERIC,
    status_code NUMERIC NOT NULL,
    status_reason bytea NOT NULL
);

CREATE INDEX ON error_transactions (block_number);
CREATE INDEX ON error_transactions (hash);
