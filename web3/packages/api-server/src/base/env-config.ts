import { env } from "process";
import dotenv from "dotenv";

dotenv.config({ path: "./.env" });

export const envConfig = {
  get databaseUrl() {
    return getRequired("DATABASE_URL");
  },
  get godwokenJsonRpc() {
    return getRequired("GODWOKEN_JSON_RPC");
  },

  _newRelicLicenseKey: getOptional("NEW_RELIC_LICENSE_KEY"),
  clusterCount: getOptional("CLUSTER_COUNT"),
  redisUrl: getOptional("REDIS_URL"),
  pgPoolMax: getOptional("PG_POOL_MAX"),
  gasPriceCacheSeconds: getOptional("GAS_PRICE_CACHE_SECONDS"),
  extraEstimateGas: getOptional("EXTRA_ESTIMATE_GAS"),
  sentryDns: getOptional("SENTRY_DNS"),
  sentryEnvironment: getOptional("SENTRY_ENVIRONMENT"),
  godwokenReadonlyJsonRpc: getOptional("GODWOKEN_READONLY_JSON_RPC"),
  enableCacheEthCall: getOptional("ENABLE_CACHE_ETH_CALL"),
  enableCacheEstimateGas: getOptional("ENABLE_CACHE_ESTIMATE_GAS"),
  enableCacheExecuteRawL2Tx: getOptional("ENABLE_CACHE_EXECUTE_RAW_L2_TX"),
  logLevel: getOptional("LOG_LEVEL"),
  logFormat: getOptional("LOG_FORMAT"),
  logRequestBody: getOptional("WEB3_LOG_REQUEST_BODY"),
  port: getOptional("PORT"),
  maxQueryNumber: getOptional("MAX_QUERY_NUMBER"),
  maxQueryTimeInMilliseconds: getOptional("MAX_QUERY_TIME_MILSECS"),
  enableProfRpc: getOptional("ENABLE_PROF_RPC"),
  enablePriceOracle: getOptional("ENABLE_PRICE_ORACLE"),
  priceOracleDiffThreshold: getOptional("PRICE_ORACLE_DIFF_THRESHOLD"),
  priceOraclePollInterval: getOptional("PRICE_ORACLE_POLL_INTERVAL"),
  priceOracleUpdateWindow: getOptional("PRICE_ORACLE_UPDATE_WINDOW"),
  gasPriceDivider: getOptional("GAS_PRICE_DIVIDER"),
  minGasPriceUpperLimit: getOptional("MIN_GAS_PRICE_UPPER_LIMIT"),
  minGasPriceLowerLimit: getOptional("MIN_GAS_PRICE_LOWER_LIMIT"),
  blockCongestionGasUsed: getOptional("BLOCK_CONGESTION_GAS_USED"),
};

function getRequired(name: string): string {
  const value = env[name];
  if (value == null) {
    throw new Error(`no env ${name} provided`);
  }

  return value;
}

function getOptional(name: string): string | undefined {
  return env[name];
}
