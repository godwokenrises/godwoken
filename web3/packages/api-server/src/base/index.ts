import { envConfig } from "./env-config";
import { GwConfig } from "./gw-config";
import { CKBPriceOracle } from "../price-oracle";

export const gwConfig: GwConfig = new GwConfig(envConfig.godwokenJsonRpc);
export const readonlyGwConfig: GwConfig = !!envConfig.godwokenReadonlyJsonRpc
  ? new GwConfig(envConfig.godwokenReadonlyJsonRpc)
  : gwConfig;

export const readonlyPriceOracle = new CKBPriceOracle({ readonly: true });
