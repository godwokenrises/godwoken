import * as modules from "./modules";
import { Callback } from "./types";
import { isRpcError, RpcError } from "./error";

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
            if (isRpcError(err)) {
              const error = {
                code: err.code,
                message: err.message,
              } as RpcError;

              if (err.data) {
                error.data = err.data;
              }

              // hotfix https://github.com/godwokenrises/godwoken/issues/1012
              // if (err.extra) {
              //   error.extra = err.extra;
              // }

              cb(error);
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
