#ifndef POLYJUICE_GLOBALS_H
#define POLYJUICE_GLOBALS_H

#define POLYJUICE_VERSION "v1.5.2"

#define ETH_ADDRESS_LEN 20

/* Key type for ETH Address Registry */
#define GW_ACCOUNT_SCRIPT_HASH_TO_ETH_ADDR 200
#define ETH_ADDR_TO_GW_ACCOUNT_SCRIPT_HASH 201

/** Polyjuice contract account (normal/create2) script args size */
#define CONTRACT_ACCOUNT_SCRIPT_ARGS_LEN 56 /* 32 + 4 + 20 */
#define CREATOR_SCRIPT_ARGS_LEN 36			/* 32 + 4 */

static uint8_t g_rollup_script_hash[32] = {0};
static uint32_t g_sudt_id = UINT32_MAX;

/**
 * Receipt.contractAddress is the created contract,
 * if the transaction was a contract creation, otherwise null
 */
static uint8_t g_created_address[20] = {0};
static uint32_t g_created_id = UINT32_MAX;

/**
 * @brief chain_id in Godwoken RollupConfig
 */
static uint64_t g_chain_id = UINT64_MAX;
/**
 * creator_account, known as root account
 * @see https://github.com/nervosnetwork/godwoken/blob/develop/docs/life_of_a_polyjuice_transaction.md#root-account--deployment
 */
static uint32_t g_creator_account_id = UINT32_MAX;

static evmc_address g_tx_origin = {0};

static uint8_t g_script_code_hash[32] = {0};
static uint8_t g_script_hash_type = 0xff;

#define UINT128_MAX uint128_t(__int128_t(-1L));
static uint128_t g_gas_price = UINT128_MAX;
/**
 * If g_eoa_transfer_flag = true, then this is an EOA transfer transaction.
 * And, g_eoa_transfer_to_address should be set.
 */
static bool g_eoa_transfer_flag = false;
static evmc_address g_eoa_transfer_to_address = {0};

/* Minimal gas of a normal transaction*/
#define MIN_TX_GAS                      21000
/* Minimal gas of a transaction that creates a contract */
#define MIN_CONTRACT_CREATION_TX_GAS    53000
/* Gas per byte of non zero data attached to a transaction */
#define	DATA_NONE_ZERO_TX_GAS           16
/* Gas per byte of data attached to a transaction */
#define	DATA_ZERO_TX_GAS                4
/* Gas of new account creation*/
#define NEW_ACCOUNT_GAS                 25000

#endif // POLYJUICE_GLOBALS_H
