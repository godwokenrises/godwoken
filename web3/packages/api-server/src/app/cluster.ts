import { startOpentelemetry } from "../opentelemetry";
// Start before logger
startOpentelemetry();

import cluster from "cluster";
import { cpus } from "os";
import { envConfig } from "../base/env-config";
import { logger } from "../base/logger";
import { BlockEmitter } from "../block-emitter";
import { CKBPriceOracle } from "../price-oracle";
import { initSentry } from "../sentry";

const numCPUs = cpus().length;
const clusterCount = +(envConfig.clusterCount || 0);
const numOfCluster = clusterCount || numCPUs;

if (cluster.isMaster) {
  logger.info(`Master ${process.pid} is running`);

  initSentry();

  // Fork workers.
  for (let i = 0; i < numOfCluster; i++) {
    cluster.fork();
  }

  const blockEmitter = new BlockEmitter();
  blockEmitter.startForever();

  if (envConfig.enablePriceOracle == "true") {
    const ckbPriceOracle = new CKBPriceOracle();
    ckbPriceOracle.startForever();
  }

  cluster.on("exit", (worker, _code, _signal) => {
    if (worker.process.exitCode === 0) {
      logger.warn(
        `Worker ${worker.id} (pid: ${worker.process.pid}) died peacefully...`
      );
    } else {
      logger.error(
        `Worker ${worker.id} (pid: ${worker.process.pid}) died with exit code ${worker.process.exitCode}, restarting it`
      );
      cluster.fork();
    }
  });
} else {
  require("./www");

  logger.info(`Worker ${process.pid} started`);
}
