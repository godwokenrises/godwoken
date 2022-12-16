import * as Sentry from "@sentry/node";
import { envConfig } from "./env-config";
import { logger } from "./logger";
const newrelic = require("newrelic");

const POLL_TIME_INTERVAL = 5000; // 5s
const LIVENESS_CHECK_INTERVAL = 5000; // 5s

// TODO: use the following class to rewrite BlockEmitter
export class BaseWorker {
  protected isRunning: boolean;
  protected pollTimeInterval: number;
  protected livenessCheckInterval: number;
  private intervalHandler: NodeJS.Timer | undefined;

  constructor({
    pollTimeInterval = POLL_TIME_INTERVAL,
    livenessCheckInterval = LIVENESS_CHECK_INTERVAL,
  } = {}) {
    this.isRunning = false;
    this.pollTimeInterval = pollTimeInterval;
    this.livenessCheckInterval = livenessCheckInterval;
  }

  // Main worker
  async startForever() {
    this.start();
    this.intervalHandler = setInterval(async () => {
      if (!this.running()) {
        logger.error(
          `${this.constructor.name} has stopped, maybe check the log?`
        );
        this.start();
      }
    }, this.livenessCheckInterval);
  }

  async stopForever() {
    this.stop();
    if (this.intervalHandler != null) {
      clearInterval(this.intervalHandler);
      logger.debug(`call ${this.constructor.name} to stop forever`);
    }
  }

  start() {
    this.isRunning = true;
    this.scheduleLoop();
  }

  stop() {
    this.isRunning = false;
  }

  running() {
    return this.isRunning;
  }

  protected scheduleLoop(ms?: number) {
    setTimeout(() => {
      this.loop();
    }, ms);
  }

  protected loop() {
    if (!this.running()) {
      return;
    }

    this.poll()
      .then((timeout) => {
        this.scheduleLoop(timeout);
      })
      .catch((e) => {
        logger.error(
          `[${this.constructor.name}] Error occurs: ${e} ${e.stack}, stopping polling!`
        );
        if (envConfig.sentryDns) {
          Sentry.captureException(e);
        }
        this.stop();
      });
  }

  protected async poll() {
    // add new relic background transaction
    return newrelic.startBackgroundTransaction(
      `${this.constructor.name}#workerPoll`,
      async () => {
        newrelic.getTransaction();
        try {
          return await this.executePoll();
        } catch (error) {
          throw error;
        } finally {
          newrelic.endTransaction();
        }
      }
    );
  }

  protected async executePoll(): Promise<number> {
    return this.pollTimeInterval;
  }
}
