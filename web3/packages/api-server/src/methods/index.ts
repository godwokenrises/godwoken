import * as modules from "./modules";
import { Callback } from "./types";
import * as Sentry from "@sentry/node";
import { INVALID_PARAMS } from "./error-code";
import { isRpcError, RpcError } from "./error";
import { envConfig } from "../base/env-config";
const newrelic = require("newrelic");

/**
 * get all methods. e.g., getBlockByNumber in eth module
 * @private
 * @param  {Object}   mod
 * @return {string[]}
 */
function getMethodNames(mod: any): string[] {
  return Object.getOwnPropertyNames(mod.prototype);
}

export interface ModConstructorArgs {
  [modName: string]: any[];
}

/**
 * return all the methods in all module
 */
function getMethods(argsList: ModConstructorArgs = {}) {
  const methods: any = {};

  modules.list.forEach((modName: string) => {
    const args = argsList[modName.toLowerCase()] || [];
    const mod = new (modules as any)[modName](...args);
    getMethodNames((modules as any)[modName])
      .filter(
        (methodName: string) =>
          methodName !== "constructor" && !methodName.startsWith("_") // exclude private method
      )
      .forEach((methodName: string) => {
        const concatedMethodName = `${modName.toLowerCase()}_${methodName}`;
        methods[concatedMethodName] = async (args: any[], cb: Callback) => {
          try {
            const result = await mod[methodName].bind(mod)(args);
            return cb(null, result);
          } catch (err: any) {
            if (envConfig.sentryDns && err.code !== INVALID_PARAMS) {
              Sentry.captureException(err, {
                extra: { method: concatedMethodName, params: args },
              });
            }

            if (isRpcError(err)) {
              const error = {
                code: err.code,
                message: err.message,
              } as RpcError;

              if (err.data) {
                error.data = err.data;
              }

              if (err.extra) {
                error.extra = err.extra;
              }

              cb(error);
              // NOTE: Our error responses are not automatically collected by NewRelic because we use Jayson instead of
              // express' error handler.
              //
              // Note: In order to link errors to transaction traces, we pass linking metadata.
              newrelic.noticeError(err, newrelic.getLinkingMetadata());
            } else {
              throw err;
            }
          }
        };
      });
  });

  // console.log(methods);
  return methods;
}

const instantFinalityHackMode = true;

export const methods = getMethods();
export const instantFinalityHackMethods = getMethods({
  eth: [instantFinalityHackMode],
});
