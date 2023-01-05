import * as Sentry from "@sentry/node";
import cluster from "cluster";
import { envConfig } from "./base/env-config";
import { logger } from "./base/logger";

export function initSentry() {
  if (envConfig.sentryDns) {
    Sentry.init({
      dsn: envConfig.sentryDns,
      environment: envConfig.sentryEnvironment || "development",
      ignoreErrors: [/^invalid nonce of account/, /^query returned more than/],
    });
    const processType = cluster.isMaster ? "master" : "cluster";
    logger.info(`Sentry init in ${processType} !!!`);
  }
}
