import { envConfig } from "../../base/env-config";

const enableList = ["Eth", "Web3", "Net", "Gw", "Poly", "Debug"];
if (envConfig.enableProfRpc === "true") {
  enableList.push("Prof");
}

export const list = enableList;

export * from "./eth";
export * from "./web3";
export * from "./net";
export * from "./gw";
export * from "./poly";
export * from "./prof";
export * from "./debug";
