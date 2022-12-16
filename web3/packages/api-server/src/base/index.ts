import { envConfig } from "./env-config";
import { GwConfig } from "./gw-config";
import { CKBPriceOracle } from "../price-oracle";

export const gwConfig = new GwConfig(envConfig.godwokenJsonRpc);

export const readonlyPriceOracle = new CKBPriceOracle({ readonly: true });
