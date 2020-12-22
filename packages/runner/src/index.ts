import { RPC } from "ckb-js-toolkit";
import { Indexer } from "@ckb-lumos/indexer";
import { Config, ChainService } from "@ckb-godwoken/godwoken";
import { DeploymentConfig } from "./config";
import { Runner } from "./runner";

const CKB_RPC_URL = "http://127.0.0.1:8114";
const indexer = new Indexer(CKB_RPC_URL, "./indexed-data");
indexer.startForever();

const rpc = new RPC(CKB_RPC_URL);

// TODO: deal with config setup later. A tool should commit genesis on chain, after
// that, godwoken should only load genesis block, and sync from there.
const chainConfig = ("TODO" as unknown) as Config;
const chainService = new ChainService(chainConfig);

const deploymentConfig = ("TODO" as unknown) as DeploymentConfig;

(async () => {
  const runner = new Runner(rpc, indexer, chainService, deploymentConfig);
  await runner.start();
})().catch((e) => {
  console.error(`Error occurs: ${e}`);
});
