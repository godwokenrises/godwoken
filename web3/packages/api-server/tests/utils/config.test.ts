import { HexString, Script } from "@ckb-lumos/base";
import {
  BackendType,
  EoaScriptType,
  GodwokenClient,
  GwScriptType,
  NodeInfo,
  NodeMode,
} from "@godwoken-web3/godwoken";
import test from "ava";
import { GwConfig } from "../../src/base/gw-config";

const script: Script = {
  code_hash: "0x",
  hash_type: "type",
  args: "0x",
};

const gwConfig = new GwConfig("http://host:8119");

let mockRpc: GodwokenClient = gwConfig.rpc;

mockRpc.getAccountIdByScriptHash = async (scriptHash: HexString) => {
  switch (scriptHash) {
    case "0x5df8df09ec23819836b888f575ca4154a2af1f1d4720bca91a5fc9f5f7d9921f":
      return 3;

    case "0x7df8df09ec23819836b888f575ca4154a2af1f1d4720bca91a5fc9f5f7d9921d":
      return 4;

    case "0xb5f81e2d10af9600194606989583ae8cc3fcb822a24fdea95f42da5ea18606da":
      return 2;

    default:
      throw new Error(
        `getAccountIdByScriptHash not mock for script hash ${scriptHash}`
      );
  }
};

mockRpc.getScriptHash = async (accountId: number) => {
  switch (accountId) {
    case 4:
      return "0x7df8df09ec23819836b888f575ca4154a2af1f1d4720bca91a5fc9f5f7d9921d";

    case 2:
      return "0xb5f81e2d10af9600194606989583ae8cc3fcb822a24fdea95f42da5ea18606da";

    case 3:
      return "0x5df8df09ec23819836b888f575ca4154a2af1f1d4720bca91a5fc9f5f7d9921f";

    default:
      throw new Error(`getScriptHash not mock for account id ${accountId}`);
  }
};

mockRpc.getScript = async (scriptHash: HexString) => {
  switch (scriptHash) {
    case "0x31e7f492d2b22220cad86b7cef30a45ce8df34b00a8ba0d0c5dfd92e7392023a":
      return {
        code_hash:
          "0x9b599c7df5d7b813f7f9542a5c8a0c12b65261a081b1dba02c2404802f772a15",
        hash_type: "type",
        args: "0x4ed4a999f0046230d67503c07f1e64f2b2ad1440f758ebfc97282be40f74673c00000010000003",
      };

    case "0x5df8df09ec23819836b888f575ca4154a2af1f1d4720bca91a5fc9f5f7d9921f":
      return {
        code_hash:
          "0x1272c80507fe5e6cf33cf3e5da6a5f02430de40abb14410ea0459361bf74ebe0",
        hash_type: "type",
        args: "0x4ed4a999f0046230d67503c07f1e64f2b2ad1440f758ebfc97282be40f74673c0xFb2C72d3ffe10Ef7c9960272859a23D24db9e04A",
      };

    default:
      throw new Error(`getScript not mock for scriptHash ${scriptHash}`);
  }
};

mockRpc.getNodeInfo = async () => {
  const nodeInfo: NodeInfo = {
    backends: [
      {
        generator_checksum: "",
        validator_script_type_hash:
          "0x32923ebad8e5417ae072decc89774324ec4a623f57af5cee6e2901d29d8e6691",
        backend_type: BackendType.Meta,
      },
      {
        generator_checksum: "",
        validator_script_type_hash:
          "0x9b599c7df5d7b813f7f9542a5c8a0c12b65261a081b1dba02c2404802f772a15",
        backend_type: BackendType.Polyjuice,
      },
      {
        generator_checksum: "",
        validator_script_type_hash:
          "0x696447c51fdb84d0e59850b26bc431425a74daaac070f2b14f5602fbb469912a",
        backend_type: BackendType.Sudt,
      },
      {
        generator_checksum: "",
        validator_script_type_hash:
          "0x59ecd45fc257a761d992507ef2e1acccf43221567f6cf3b1fc6fb9352a7a0ca3",
        backend_type: BackendType.EthAddrReg,
      },
    ],
    eoa_scripts: [
      {
        type_hash:
          "0x1272c80507fe5e6cf33cf3e5da6a5f02430de40abb14410ea0459361bf74ebe0",
        script,
        eoa_type: EoaScriptType.Eth,
      },
    ],
    gw_scripts: [
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.Deposit,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.Withdraw,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.StateValidator,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.StakeLock,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.CustodianLock,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.ChallengeLock,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.L1Sudt,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.L2Sudt,
      },
      {
        type_hash:
          "0xcddb997266a74a5e940a240d63ef8aa89d116999044e421dc337ead16ea870eb",
        script,
        script_type: GwScriptType.OmniLock,
      },
    ],
    rollup_cell: {
      type_hash:
        "0x4ed4a999f0046230d67503c07f1e64f2b2ad1440f758ebfc97282be40f74673c",
      type_script: script,
    },
    rollup_config: {
      required_staking_capacity: "0x2540be400",
      challenge_maturity_blocks: "0x64",
      finality_blocks: "0x3",
      reward_burn_rate: "0x32",
      chain_id: "0x116e8",
    },
    version: "v1.1.0",
    mode: NodeMode.FullNode,
  };
  return nodeInfo;
};

test("init gw config", async (t) => {
  const config = await gwConfig.init();
  t.deepEqual(config.eoaScripts, {
    eth: {
      script,
      typeHash:
        "0x1272c80507fe5e6cf33cf3e5da6a5f02430de40abb14410ea0459361bf74ebe0",
    },
  });
  t.is(config.accounts.polyjuiceCreator.id, "0x4");
  t.is(config.accounts.defaultFrom.id, "0x3");
  t.is(config.accounts.ethAddrReg.id, "0x2");
  t.is(config.nodeMode, NodeMode.FullNode);
  t.is(config.web3ChainId, "0x116e8");
});
