// default filter cache setting
export const CACHE_EXPIRED_TIME_MILSECS = 5 * 60 * 1000; // milsec, default 5 minutes
// limit redis store filter size
export const MAX_FILTER_TOPIC_ARRAY_LENGTH = 20;

// The Cache Key Prfixs
export const GW_RPC_KEY = "gwRPC";

export const TX_HASH_MAPPING_PREFIX_KEY = "TxHashMapping";
export const TX_HASH_MAPPING_CACHE_EXPIRED_TIME_MILSECS = 2 * 60 * 60 * 1000; // 2 hours
export const AUTO_CREATE_ACCOUNT_PREFIX_KEY = "AutoCreateAccount";
export const AUTO_CREATE_ACCOUNT_CACHE_EXPIRED_TIME_MILSECS =
  2 * 60 * 60 * 1000; // 2 hours

export const TIP_BLOCK_HASH_CACHE_KEY = "tipBlockHash";
export const TIP_BLOCK_HASH_CACHE_EXPIRED_TIME_MS = 1000 * 60 * 5; // 5 minutes

// knex db query cache time
export const QUERY_CACHE_EXPIRED_TIME_MS = 1000 * 45; // 45 seconds ~= block produce interval
