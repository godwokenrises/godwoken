import { Script } from "@ckb-lumos/base";
import { ChainService } from "@ckb-godwoken/godwoken";
import jayson from "jayson/promise";
import cors from "cors";
import connect from "connect";
import { json } from "body-parser";
import { Logger } from "./utils";

function isHexString(s: any) {
  return typeof s === "string" && /^0x([0-9a-fA-F][0-9a-fA-F])*$/.test(s);
}

function isHash(s: any) {
  return isHexString(s) && s.length === 66;
}

export class JsonrpcServer {
  chainService: ChainService;
  server: jayson.Server;
  logger: Logger;
  listen: string;

  constructor(chainService: ChainService, listen: string, logger: Logger) {
    this.chainService = chainService;
    this.server = new jayson.Server({
      gw_submitL2Transaction: this.wrapWithLogger(this.submitL2Transaction),
      gw_executeL2Tranaction: this.wrapWithLogger(this.executeL2Transaction),
      gw_submitWithdrawalRequest: this.wrapWithLogger(
        this.submitWithdrawalRequest
      ),
      gw_getBalance: this.wrapWithLogger(this.getBalance),
      gw_getStorageAt: this.wrapWithLogger(this.getStorageAt),
      gw_getAccountIdByScriptHash: this.wrapWithLogger(
        this.getAccountIdByScriptHash
      ),
      gw_getNonce: this.wrapWithLogger(this.getNonce),
      gw_getScript: this.wrapWithLogger(this.getScript),
      gw_getScriptHash: this.wrapWithLogger(this.getScriptHash),
      gw_getData: this.wrapWithLogger(this.getData),
      gw_getDataHash: this.wrapWithLogger(this.getDataHash),
    });
    this.listen = listen;
    this.logger = logger;
  }

  wrapWithLogger(f: Function) {
    return async (args: any) => {
      try {
        return await f.bind(this)(args);
      } catch (e) {
        this.logger("error", `Error: ${e} ${e.stack}`);
        return this.server.error(502, "Internal error, check server logs!");
      }
    };
  }

  async submitL2Transaction(args: any) {
    if (args.length !== 1 || !isHexString(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.submitL2Transaction(args[0]);
  }

  async executeL2Transaction(args: any) {
    if (args.length !== 1 || !isHexString(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.execute(args[0]);
  }

  async submitWithdrawalRequest(args: any) {
    if (args.length !== 1 || !isHexString(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    await this.chainService.submitWithdrawalRequest(args[0]);
    return "OK";
  }

  async getBalance(args: any) {
    if (
      args.length !== 2 ||
      typeof args[0] !== "number" ||
      typeof args[1] !== "number"
    ) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getBalance(args[0], args[1]);
  }

  async getStorageAt(args: any) {
    if (args.length !== 2 || typeof args[0] !== "number" || !isHash(args[1])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getStorageAt(args[0], args[1]);
  }

  async getAccountIdByScriptHash(args: any) {
    if (args.length !== 1 || !isHash(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getAccountIdByScriptHash(args[0]);
  }

  async getNonce(args: any) {
    if (args.length !== 1 || typeof args[0] !== "number") {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getNonce(args[0]);
  }

  async getScriptHash(args: any) {
    if (args.length !== 1 || typeof args[0] !== "number") {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getScriptHash(args[0]);
  }

  async getScript(args: any): Promise<Script | undefined> {
    if (args.length !== 1 || !isHash(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getScript(args[0]);
  }

  async getData(args: any) {
    if (args.length !== 1 || !isHash(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getData(args[0]);
  }

  async getDataHash(args: any) {
    if (args.length !== 1 || !isHash(args[0])) {
      throw this.server.error(501, "Invalid arguments!");
    }
    return await this.chainService.getDataHash(args[0]);
  }

  async start() {
    const app = connect();
    app.use(cors({ methods: ["GET", "PUT", "POST"] }));
    app.use(json());
    app.use(this.server.middleware());

    app.listen(this.listen);
  }
}
