import { HexNumber, HexString } from "@ckb-lumos/base";
import { GodwokenClient } from "@godwoken-web3/godwoken";
import { gwConfig } from "../base";
import { EthRegistryAddress } from "../base/address";
import { isGaslessTransaction } from "../gasless/utils";
import { calcIntrinsicGas } from "../util";
import { CKB_SUDT_ID, POLY_MAX_BLOCK_GAS_LIMIT } from "./constant";
import { TransactionCallObject } from "./types";
import {
  verifyEnoughBalance,
  verifyGaslessTransaction,
  verifyGasLimit,
  verifyIntrinsicGas,
} from "./validator";

export class EthNormalizer {
  private rpc: GodwokenClient;

  constructor(rpc: GodwokenClient) {
    this.rpc = rpc;
  }

  async normalizeCallTx(
    txCallObj: TransactionCallObject
  ): Promise<Required<TransactionCallObject>> {
    const value = txCallObj.value || "0x0";
    const data = txCallObj.data || "0x";
    const toAddress = txCallObj.to;
    const fromAddress =
      txCallObj.from || (await getDefaultFromAddress(this.rpc));

    // we should set default price to 0 instead of minGasPrice,
    // otherwise read operation might fail the balance check.
    const gasPrice = txCallObj.gasPrice || "0x0";

    // set default gas limit to min(maxBlockGas, userBalanceAvailableGas)
    // TODO: use real blockAvailableGas to replace POLY_MAX_BLOCK_GAS_LIMIT
    const maxBlockGasLimit =
      "0x" + BigInt(POLY_MAX_BLOCK_GAS_LIMIT).toString(16);
    let gas = txCallObj.gas;
    if (gas == null) {
      gas =
        +gasPrice === 0
          ? maxBlockGasLimit
          : min(
              maxBlockGasLimit,
              await getMaxGasByBalance(this.rpc, fromAddress, gasPrice, value)
            );
    }

    const gasLimitErr = verifyGasLimit(gas, 0);
    if (gasLimitErr) {
      throw gasLimitErr.padContext(this.normalizeCallTx.name);
    }

    // only check if it is gasless transaction when entrypointContract is configured
    if (
      gwConfig.entrypointContract != null &&
      isGaslessTransaction(
        { to: toAddress, gasPrice, data },
        gwConfig.entrypointContract
      )
    ) {
      const err = verifyGaslessTransaction(toAddress, data, gasPrice, gas, 0);
      if (err) {
        throw err.padContext(this.normalizeCallTx.name);
      }
    }

    const intrinsicGasErr = verifyIntrinsicGas(toAddress, data, gas, 0);
    if (intrinsicGasErr) {
      throw intrinsicGasErr.padContext(this.normalizeCallTx.name);
    }

    // check if from address have enough balance
    // when gasPrice in ethCallObj is provided.
    if (txCallObj.gasPrice != null) {
      const balanceErr = await verifyEnoughBalance(
        this.rpc,
        fromAddress,
        value,
        gas,
        gasPrice,
        0
      );
      if (balanceErr) {
        throw balanceErr.padContext(
          `${this.normalizeCallTx.name}: from account ${fromAddress}`
        );
      }
    }

    return {
      value,
      data,
      to: toAddress,
      from: fromAddress,
      gas,
      gasPrice,
    };
  }

  async normalizeEstimateGasTx(
    txEstimateGasObj: Partial<TransactionCallObject>
  ): Promise<Required<TransactionCallObject>> {
    const data = txEstimateGasObj.data || "0x";
    const toAddress = txEstimateGasObj.to || "0x";
    const fromAddress =
      txEstimateGasObj.from || (await getDefaultFromAddress(this.rpc));
    const gasPrice = txEstimateGasObj.gasPrice || "0x0";
    const value = txEstimateGasObj.value || "0x0";

    // TODO: use real blockAvailableGas to replace POLY_MAX_BLOCK_GAS_LIMIT
    const maxBlockGasLimit =
      "0x" + BigInt(POLY_MAX_BLOCK_GAS_LIMIT).toString(16);

    // normalize the gas limit
    let gas = txEstimateGasObj.gas || maxBlockGasLimit;

    // check gas-limit lower bound
    const intrinsicGas = calcIntrinsicGas(toAddress, data);
    const gasLow = "0x" + intrinsicGas.toString(16);
    if (BigInt(gas) < BigInt(gasLow)) {
      gas = gasLow;
    }

    //check gasless transaction
    if (
      gwConfig.entrypointContract != null &&
      isGaslessTransaction(
        { to: toAddress, gasPrice, data },
        gwConfig.entrypointContract
      )
    ) {
      const err = verifyGaslessTransaction(toAddress, data, gasPrice, gas, 0);
      if (err) {
        throw err.padContext(this.normalizeCallTx.name);
      }
    }

    // check gas-limit cap with user available gas
    if (BigInt(gasPrice) > 0n) {
      const gasCap = await getMaxGasByBalance(
        this.rpc,
        fromAddress,
        gasPrice,
        value
      );

      if (BigInt(gasCap) < BigInt(gasLow)) {
        throw new Error(
          `balance available gas ${gasCap} not enough for minimal required ${gasLow}`
        );
      }

      if (BigInt(gas) > BigInt(gasCap)) {
        gas = gasCap;
      }
    }

    return {
      value,
      data,
      to: toAddress,
      from: fromAddress,
      gas,
      gasPrice,
    };
  }
}

async function getDefaultFromAddress(rpc: GodwokenClient): Promise<HexString> {
  const defaultFromScript = await rpc.getScript(
    gwConfig.accounts.defaultFrom.scriptHash
  );
  if (defaultFromScript == null) {
    throw new Error("default from script is null");
  }
  const defaultFromAddress = "0x" + defaultFromScript.args.slice(2).slice(64);
  return defaultFromAddress;
}

export async function getMaxGasByBalance(
  rpc: GodwokenClient,
  from: HexString,
  gasPrice: HexNumber,
  txValue: HexNumber = "0x0"
): Promise<HexNumber> {
  if (gasPrice === "0x" || gasPrice === "0x0") {
    throw new Error(`[${getMaxGasByBalance.name}] gasPrice should > 0`);
  }

  const registryAddress: EthRegistryAddress = new EthRegistryAddress(from);
  const balance = await rpc.getBalance(
    registryAddress.serialize(),
    +CKB_SUDT_ID
  );

  if (balance < BigInt(txValue)) {
    throw new Error(
      `[${getMaxGasByBalance.name}] insufficient funds for transfer`
    );
  }

  const availableBalance = balance - BigInt(txValue);
  const maxGas = availableBalance / BigInt(gasPrice);
  return "0x" + maxGas.toString(16);
}

export function min(...values: HexNumber[]): HexNumber {
  const num = values.reduce((previousValue, currentValue) =>
    BigInt(currentValue) < BigInt(previousValue) ? currentValue : previousValue
  );
  return num;
}
