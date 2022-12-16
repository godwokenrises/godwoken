// source: https://github.com/nervosnetwork/godwoken-polyjuice/blob/main/docs/EVM-compatible.md
export const POLY_MAX_BLOCK_GAS_LIMIT = 12500000;
export const POLY_MAX_TRANSACTION_GAS_LIMIT = 12500000;
export const POLY_BLOCK_DIFFICULTY = BigInt("2500000000000000");

export const RPC_MAX_GAS_LIMIT = 50000000;

export const TX_GAS = 21000;
export const TX_GAS_CONTRACT_CREATION = 53000;
export const TX_DATA_ZERO_GAS = 4;
export const TX_DATA_NONE_ZERO_GAS = 16;

export const ZERO_ETH_ADDRESS = `0x${"0".repeat(40)}`;
export const DEFAULT_LOGS_BLOOM = "0x" + "00".repeat(256);

export const POLYJUICE_SYSTEM_PREFIX = 255;
export const POLYJUICE_CONTRACT_CODE = 1;
// export const POLYJUICE_DESTRUCTED = 2;
// export const GW_KEY_BYTES = 32;
export const GW_ACCOUNT_KV = 0;
export const CKB_SUDT_ID = "0x1";
export const META_CONTRACT_ID = "0x0";
export const SUDT_OPERATION_LOG_FLAG = "0x0";
export const SUDT_PAY_FEE_LOG_FLAG = "0x1";
export const POLYJUICE_SYSTEM_LOG_FLAG = "0x2";
export const POLYJUICE_USER_LOG_FLAG = "0x3";

export const HEADER_NOT_FOUND_ERR_MESSAGE = "header not found";

export const COMPATIBLE_DOCS_URL =
  "https://github.com/nervosnetwork/godwoken-web3/blob/main/docs/compatibility.md";

// 128kb
// see also https://github.com/ethereum/go-ethereum/blob/b3b8b268eb585dfd3c1c9e9bbebc55968f3bec4b/core/tx_pool.go#L43-L53
export const MAX_TRANSACTION_SIZE = BigInt("131072");

// https://github.com/nervosnetwork/godwoken/blob/develop/crates/eoa-mapping/src/eth_register.rs#L16
export const MAX_ADDRESS_SIZE_PER_REGISTER_BATCH = 50;

export const AUTO_CREATE_ACCOUNT_FROM_ID = "0x0";

// if sync behind 3 blocks, something went wrong
export const MAX_ALLOW_SYNC_BLOCKS_DIFF = 3;
