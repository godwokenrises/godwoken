import { utils, HexNumber, Script, HexString } from "@ckb-lumos/base";
import {
  BackendInfo,
  EoaScript,
  GwScript,
  NodeInfo,
  RollupCell,
  RollupConfig,
} from "./types/node-info";
import {
  NodeMode,
  BackendType,
  EoaScriptType,
  GwScriptType,
  GodwokenClient,
  NodeInfo as GwNodeInfo,
} from "@godwoken-web3/godwoken";
import { CKB_SUDT_ID } from "../methods/constant";
import { Uint32 } from "./types/uint";
import { snakeToCamel } from "../util";
import { EntryPointContract } from "../gasless/entrypoint";

// source: https://github.com/nervosnetwork/godwoken/commit/d6c98d8f8a199b6ec29bc77c5065c1108220bb0a#diff-c56fda2ca3b1366049c88e633389d9b6faa8366151369fd7314c81f6e389e5c7R5
const BUILTIN_ETH_ADDR_REG_ACCOUNT_ID = 2;

export class GwConfig {
  rpc: GodwokenClient;
  private iNodeInfo: NodeInfo | undefined;
  private iWeb3ChainId: HexNumber | undefined;
  private iAccounts: ConfigAccounts | undefined;
  private iEoaScripts: ConfigEoaScripts | undefined;
  private iBackends: ConfigBackends | undefined;
  private iGwScripts: ConfigGwScripts | undefined;
  private iRollupConfig: RollupConfig | undefined;
  private iRollupCell: RollupCell | undefined;
  private iNodeMode: NodeMode | undefined;
  private iNodeVersion: string | undefined;
  private iEntryPointContract: EntryPointContract | undefined;

  constructor(rpcOrUrl: GodwokenClient | string) {
    if (typeof rpcOrUrl === "string") {
      this.rpc = new GodwokenClient(rpcOrUrl);
      return;
    }

    this.rpc = rpcOrUrl;
  }

  async init(): Promise<GwConfig> {
    this.iNodeInfo = await this.getNodeInfoFromRpc();

    const ethAddrReg = await this.fetchEthAddrRegAccount();
    const creator = await this.fetchCreatorAccount();
    const defaultFrom = await this.fetchDefaultFromAccount();

    this.iAccounts = {
      polyjuiceCreator: creator,
      ethAddrReg,
      defaultFrom,
    };

    this.iEoaScripts = toConfigEoaScripts(this.nodeInfo);
    this.iGwScripts = toConfigGwScripts(this.nodeInfo);
    this.iBackends = toConfigBackends(this.nodeInfo);
    this.iWeb3ChainId = this.nodeInfo.rollupConfig.chainId;
    this.iRollupCell = this.nodeInfo.rollupCell;
    this.iRollupConfig = this.nodeInfo.rollupConfig;
    this.iNodeMode = this.nodeInfo.mode;
    this.iNodeVersion = this.nodeInfo.version;

    const entrypointAddr = this.nodeInfo.gaslessTxSupport?.entrypointAddress;
    if (entrypointAddr != null) {
      this.iEntryPointContract = new EntryPointContract(
        this.rpc,
        entrypointAddr,
        ethAddrReg.id
      );
      await this.iEntryPointContract.init();
    }

    return this;
  }

  public get web3ChainId(): HexNumber {
    return this.iWeb3ChainId!;
  }

  public get accounts(): ConfigAccounts {
    return this.iAccounts!;
  }

  public get backends(): ConfigBackends {
    return this.iBackends!;
  }

  public get eoaScripts(): ConfigEoaScripts {
    return this.iEoaScripts!;
  }

  public get gwScripts(): ConfigGwScripts {
    return this.iGwScripts!;
  }

  public get rollupConfig(): RollupConfig {
    return this.iRollupConfig!;
  }

  public get rollupCell(): RollupCell {
    return this.iRollupCell!;
  }

  public get nodeMode(): NodeMode {
    return this.iNodeMode!;
  }

  public get nodeVersion(): string {
    return this.iNodeVersion!;
  }

  public get entrypointContract(): EntryPointContract | undefined {
    return this.iEntryPointContract;
  }

  private get nodeInfo(): NodeInfo {
    return this.iNodeInfo!;
  }

  private async getNodeInfoFromRpc() {
    const nodeInfo = await this.rpc.getNodeInfo();
    return toApiNodeInfo(nodeInfo);
  }

  private async fetchCreatorAccount() {
    const ckbSudtId = new Uint32(+CKB_SUDT_ID).toLittleEndian();

    const creatorScriptArgs =
      this.nodeInfo.rollupCell.typeHash + ckbSudtId.slice(2);

    const polyjuiceValidatorTypeHash = this.nodeInfo.backends.find(
      (b) => b.backendType === BackendType.Polyjuice
    )?.validatorScriptTypeHash;

    if (polyjuiceValidatorTypeHash == null) {
      throw new Error(
        `[GwConfig => fetchCreatorAccount] polyjuiceValidatorTypeHash is null! ${JSON.stringify(
          this.nodeInfo.backends,
          null,
          2
        )}`
      );
    }

    const script: Script = {
      code_hash: polyjuiceValidatorTypeHash,
      hash_type: "type",
      args: creatorScriptArgs,
    };

    const scriptHash = utils.computeScriptHash(script);

    const creatorId = await this.rpc.getAccountIdByScriptHash(scriptHash);
    if (creatorId == null) {
      throw new Error(
        `[${
          GwConfig.name
        }] can't find creator account id by script hash ${scriptHash}, script detail: ${JSON.stringify(
          script,
          null,
          2
        )}`
      );
    }
    const creatorIdHex = "0x" + BigInt(creatorId).toString(16);
    return new Account(creatorIdHex, scriptHash);
  }

  private async fetchEthAddrRegAccount() {
    const registryScriptArgs = this.nodeInfo.rollupCell.typeHash;

    const ethAddrRegValidatorTypeHash = this.nodeInfo.backends.find(
      (b) => b.backendType === BackendType.EthAddrReg
    )?.validatorScriptTypeHash;
    if (ethAddrRegValidatorTypeHash == null) {
      throw new Error(
        `[GwConfig => fetchEthAddrRegAccount] ethAddrRegValidatorTypeHash is null! ${JSON.stringify(
          this.nodeInfo.backends,
          null,
          2
        )}`
      );
    }

    const script: Script = {
      code_hash: ethAddrRegValidatorTypeHash,
      hash_type: "type",
      args: registryScriptArgs,
    };

    const scriptHash = utils.computeScriptHash(script);

    const regId = await this.rpc.getAccountIdByScriptHash(scriptHash);
    if (regId == null) {
      throw new Error(
        `[${
          GwConfig.name
        }] can't find ethAddrReg account id by script hash ${scriptHash}, script detail: ${JSON.stringify(
          script,
          null,
          2
        )}`
      );
    }

    if (regId !== BUILTIN_ETH_ADDR_REG_ACCOUNT_ID) {
      throw new Error(
        `[${
          GwConfig.name
        }] ethAddrReg account id is not equal to builtin id ${BUILTIN_ETH_ADDR_REG_ACCOUNT_ID}, script detail: ${JSON.stringify(
          script,
          null,
          2
        )}`
      );
    }

    const regIdHex = "0x" + BigInt(regId).toString(16);
    return new Account(regIdHex, scriptHash);
  }

  // we search the first account id = 3, if it is eoa account, use it, otherwise continue with id + 1;
  private async fetchDefaultFromAccount() {
    const ethAccountLockTypeHash = this.nodeInfo.eoaScripts.find(
      (s) => s.eoaType === EoaScriptType.Eth
    )?.typeHash;

    if (ethAccountLockTypeHash == null) {
      throw new Error(
        `[GwConfig => fetchDefaultFromAccount] ethAccountLockTypeHash is null! ${JSON.stringify(
          this.nodeInfo.eoaScripts,
          null,
          2
        )}`
      );
    }

    const firstEoaAccount = await findFirstEoaAccountId(
      this.rpc,
      ethAccountLockTypeHash
    );

    if (firstEoaAccount == null) {
      throw new Error("can not find first eoa account.");
    }

    return firstEoaAccount;
  }
}

export class Account {
  id: HexNumber;
  scriptHash: HexString;

  constructor(id: HexNumber, scriptHash: HexString) {
    this.id = id;
    this.scriptHash = scriptHash;
  }
}

export interface ConfigAccounts {
  polyjuiceCreator: Account;
  ethAddrReg: Account;
  defaultFrom: Account;
}

export interface ConfigBackends {
  sudt: Omit<BackendInfo, "backendType">;
  meta: Omit<BackendInfo, "backendType">;
  polyjuice: Omit<BackendInfo, "backendType">;
  ethAddrReg: Omit<BackendInfo, "backendType">;
}

function toConfigBackends(nodeInfo: NodeInfo) {
  const sudt = nodeInfo.backends.filter(
    (b) => b.backendType === BackendType.Sudt
  )[0];
  const meta = nodeInfo.backends.filter(
    (b) => b.backendType === BackendType.Meta
  )[0];
  const polyjuice = nodeInfo.backends.filter(
    (b) => b.backendType === BackendType.Polyjuice
  )[0];
  const ethAddrReg = nodeInfo.backends.filter(
    (b) => b.backendType === BackendType.EthAddrReg
  )[0];

  const backends: ConfigBackends = {
    sudt: {
      validatorCodeHash: sudt.validatorCodeHash,
      generatorCodeHash: sudt.generatorCodeHash,
      validatorScriptTypeHash: sudt.validatorScriptTypeHash,
    },
    meta: {
      validatorCodeHash: meta.validatorCodeHash,
      generatorCodeHash: meta.generatorCodeHash,
      validatorScriptTypeHash: meta.validatorScriptTypeHash,
    },
    polyjuice: {
      validatorCodeHash: polyjuice.validatorCodeHash,
      generatorCodeHash: polyjuice.generatorCodeHash,
      validatorScriptTypeHash: polyjuice.validatorScriptTypeHash,
    },
    ethAddrReg: {
      validatorCodeHash: ethAddrReg.validatorCodeHash,
      generatorCodeHash: ethAddrReg.generatorCodeHash,
      validatorScriptTypeHash: ethAddrReg.validatorScriptTypeHash,
    },
  };
  return backends;
}

export interface ConfigGwScripts {
  deposit: Omit<GwScript, "scriptType">;
  withdraw: Omit<GwScript, "scriptType">;
  stateValidator: Omit<GwScript, "scriptType">;
  stakeLock: Omit<GwScript, "scriptType">;
  custodianLock: Omit<GwScript, "scriptType">;
  challengeLock: Omit<GwScript, "scriptType">;
  l1Sudt: Omit<GwScript, "scriptType">;
  l2Sudt: Omit<GwScript, "scriptType">;
  omniLock: Omit<GwScript, "scriptType">;
}

function toConfigGwScripts(nodeInfo: NodeInfo) {
  const deposit = findGwScript(GwScriptType.Deposit, nodeInfo);
  const withdraw = findGwScript(GwScriptType.Withdraw, nodeInfo);
  const stateValidator = findGwScript(GwScriptType.StateValidator, nodeInfo);
  const stakeLock = findGwScript(GwScriptType.StakeLock, nodeInfo);
  const custodianLock = findGwScript(GwScriptType.CustodianLock, nodeInfo);
  const challengeLock = findGwScript(GwScriptType.ChallengeLock, nodeInfo);
  const l1Sudt = findGwScript(GwScriptType.L1Sudt, nodeInfo);
  const l2Sudt = findGwScript(GwScriptType.L2Sudt, nodeInfo);
  const omniLock = findGwScript(GwScriptType.OmniLock, nodeInfo);

  const configGwScripts: ConfigGwScripts = {
    deposit: {
      script: deposit.script,
      typeHash: deposit.typeHash,
    },
    withdraw: {
      script: withdraw.script,
      typeHash: withdraw.typeHash,
    },
    stateValidator: {
      script: stateValidator.script,
      typeHash: stateValidator.typeHash,
    },
    stakeLock: {
      script: stakeLock.script,
      typeHash: stakeLock.typeHash,
    },
    custodianLock: {
      script: custodianLock.script,
      typeHash: custodianLock.typeHash,
    },
    challengeLock: {
      script: challengeLock.script,
      typeHash: challengeLock.typeHash,
    },
    l1Sudt: {
      script: l1Sudt.script,
      typeHash: l1Sudt.typeHash,
    },
    l2Sudt: {
      script: l2Sudt.script,
      typeHash: l2Sudt.typeHash,
    },
    omniLock: {
      script: omniLock.script,
      typeHash: omniLock.typeHash,
    },
  };
  return configGwScripts;
}

function findGwScript(type: GwScriptType, nodeInfo: NodeInfo): GwScript {
  const script = nodeInfo.gwScripts.find((s) => s.scriptType === type);
  if (script == null) {
    throw new Error(`[GwConfig => findGwScript] can not find script ${type}`);
  }
  return script;
}

export interface ConfigEoaScripts {
  eth: Omit<EoaScript, "eoaType">;
}

function toConfigEoaScripts(nodeInfo: NodeInfo) {
  const eth = nodeInfo.eoaScripts.find((e) => e.eoaType === EoaScriptType.Eth);
  if (eth == null) {
    throw new Error("no Eth eoa script!");
  }

  const configEoas: ConfigEoaScripts = {
    eth: {
      typeHash: eth.typeHash,
      script: eth.script,
    },
  };
  return configEoas;
}

function toApiNodeInfo(nodeInfo: GwNodeInfo): NodeInfo {
  // todo: use determinable converting to replace snakeToCamel
  return snakeToCamel(nodeInfo, ["code_hash", "hash_type"]);
}

async function findFirstEoaAccountId(
  rpc: GodwokenClient,
  ethAccountLockTypeHash: HexString,
  startAccountId: number = 3,
  maxTry: number = 20
) {
  for (let id = startAccountId; id < maxTry; id++) {
    const scriptHash = await rpc.getScriptHash(id);
    if (scriptHash == null) {
      continue;
    }
    const script = await rpc.getScript(scriptHash);
    if (script == null) {
      continue;
    }
    if (script.code_hash === ethAccountLockTypeHash) {
      const accountIdHex = "0x" + BigInt(id).toString(16);
      return new Account(accountIdHex, scriptHash);
    }

    await asyncSleep(500);
  }

  return null;
}

const asyncSleep = async (ms = 0) => {
  return new Promise((r) => setTimeout(() => r("ok"), ms));
};
