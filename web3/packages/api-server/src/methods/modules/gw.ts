import { handleGwError } from "../gw-error";
import {
  RPC,
  RunResult,
  GodwokenClient,
  EthAddrRegArgsType,
  BatchSetMapping,
  SetMapping,
  SudtArgsType,
  SudtTransfer,
} from "@godwoken-web3/godwoken";
import {
  middleware,
  verifyEnoughBalance,
  verifyGaslessTransaction,
  verifyGasLimit,
  verifyGasPrice,
  verifyIntrinsicGas,
  verifyL2TxFee,
} from "../validator";
import { Hash, HexNumber, HexString, Script, utils } from "@ckb-lumos/base";
import { Store } from "../../cache/store";
import { envConfig } from "../../base/env-config";
import { CACHE_EXPIRED_TIME_MILSECS, GW_RPC_KEY } from "../../cache/constant";
import { logger } from "../../base/logger";
import { DataCacheConstructor, RedisDataCache } from "../../cache/data";
import {
  decodePolyjuiceArgs,
  isPolyjuiceTransactionArgs,
  parseSerializeEthAddrRegArgs,
  parseSerializeL2Transaction,
  parseSerializeSudtArgs,
} from "../../parse-tx";
import { InvalidParamsError } from "../error";
import { gwConfig, readonlyPriceOracle } from "../../base";
import { META_CONTRACT_ID } from "../constant";
import {
  PolyjuiceTransaction,
  recoverEthAddressFromPolyjuiceTx,
} from "../../convert-tx";
import { isGaslessTransaction } from "../../gasless/utils";

export class Gw {
  private rpc: RPC;
  private readonlyRpc: RPC;
  private gwCache: Store;

  constructor() {
    this.rpc = new RPC(envConfig.godwokenJsonRpc);
    this.readonlyRpc = !!envConfig.godwokenReadonlyJsonRpc
      ? new RPC(envConfig.godwokenReadonlyJsonRpc)
      : this.rpc;

    this.gwCache = new Store(true, CACHE_EXPIRED_TIME_MILSECS);

    this.ping = middleware(this.ping.bind(this), 0);
    this.get_tip_block_hash = middleware(this.get_tip_block_hash.bind(this), 0);
    this.get_block_hash = middleware(this.get_block_hash.bind(this), 0);
    this.get_block = middleware(this.get_block.bind(this), 0);
    this.get_block_by_number = middleware(
      this.get_block_by_number.bind(this),
      0
    );
    this.get_balance = middleware(this.get_balance.bind(this), 0);
    this.get_storage_at = middleware(this.get_storage_at.bind(this), 0);
    this.get_account_id_by_script_hash = middleware(
      this.get_account_id_by_script_hash.bind(this),
      0
    );
    this.get_nonce = middleware(this.get_nonce.bind(this), 0);
    this.get_script = middleware(this.get_script.bind(this), 0);
    this.get_script_hash = middleware(this.get_script_hash.bind(this), 0);
    this.get_data = middleware(this.get_data.bind(this), 0);
    this.get_transaction_receipt = middleware(
      this.get_transaction_receipt.bind(this),
      0
    );
    this.get_transaction = middleware(this.get_transaction.bind(this), 0);
    this.execute_l2transaction = middleware(
      this.execute_l2transaction.bind(this),
      0
    );
    this.execute_raw_l2transaction = middleware(
      this.execute_raw_l2transaction.bind(this),
      0
    );
    this.submit_l2transaction = middleware(
      this.submit_l2transaction.bind(this),
      0
    );
    this.submit_withdrawal_request = middleware(
      this.submit_withdrawal_request.bind(this),
      0
    );
    this.get_last_submitted_info = middleware(
      this.get_last_submitted_info.bind(this),
      0
    );
    this.get_node_info = middleware(this.get_node_info.bind(this), 0);
    this.is_request_in_queue = middleware(
      this.is_request_in_queue.bind(this),
      0
    );
    this.get_pending_tx_hashes = middleware(
      this.get_pending_tx_hashes.bind(this),
      0
    );
    this.debug_replay_transaction = middleware(
      this.debug_replay_transaction.bind(this),
      1
    );
  }

  async ping(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_ping(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async get_tip_block_hash(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_tip_block_hash(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [block_number]
   * @returns
   */
  async get_block_hash(args: any[]) {
    try {
      args[0] = formatHexNumber(args[0]);

      const result = await this.readonlyRpc.gw_get_block_hash(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [block_hash]
   * @returns
   */
  async get_block(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_block(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [block_number]
   * @returns
   */
  async get_block_by_number(args: any[]) {
    try {
      args[0] = formatHexNumber(args[0]);

      const result = await this.readonlyRpc.gw_get_block_by_number(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [script_hash_160, sudt_id, (block_number)]
   * @returns
   */
  async get_balance(args: any[]) {
    try {
      args[1] = formatHexNumber(args[1]);
      args[2] = formatHexNumber(args[2]);

      const result = await this.readonlyRpc.gw_get_balance(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [account_id, key(Hash), (block_number)]
   * @returns
   */
  async get_storage_at(args: any[]) {
    try {
      args[0] = formatHexNumber(args[0]);
      args[2] = formatHexNumber(args[2]);

      const result = await this.readonlyRpc.gw_get_storage_at(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [script_hash]
   * @returns
   */
  async get_account_id_by_script_hash(args: any[]) {
    try {
      const scriptHash = args[0];
      let result = await this.gwCache.get(`${GW_RPC_KEY}_${scriptHash}`);
      if (result != null) {
        logger.debug(`using cache: ${scriptHash} -> ${result}`);
        return result;
      }

      result = await this.readonlyRpc.gw_get_account_id_by_script_hash(...args);
      if (result != null) {
        logger.debug(`update cache: ${scriptHash} -> ${result}`);
        this.gwCache.insert(`${GW_RPC_KEY}_${scriptHash}`, result);
      }
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [account_id, (block_number)]
   * @returns
   */
  async get_nonce(args: any[]) {
    try {
      args[0] = formatHexNumber(args[0]);
      args[1] = formatHexNumber(args[1]);

      const result = await this.rpc.gw_get_nonce(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [script_hash]
   * @returns
   */
  async get_script(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_script(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [account_id]
   * @returns
   */
  async get_script_hash(args: any[]) {
    try {
      args[0] = formatHexNumber(args[0]);

      const result = await this.readonlyRpc.gw_get_script_hash(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [data_hash, (block_number)]
   * @returns
   */
  async get_data(args: any[]) {
    try {
      args[1] = formatHexNumber(args[1]);

      const result = await this.readonlyRpc.gw_get_data(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [tx_hash]
   * @returns
   */
  async get_transaction_receipt(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_transaction_receipt(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [tx_hash, (verbose:number)]
   * @returns
   */
  async get_transaction(args: any[]) {
    try {
      const txWithStatus = await this.readonlyRpc.gw_get_transaction(...args);
      // if verbose = 0 (default is 0) && tx_with_status is null, get transaction from fullnode.
      const verbose = args[1];
      if (
        (verbose == null || verbose === 0) &&
        txWithStatus == null &&
        this.readonlyRpc !== this.rpc
      ) {
        return await this.rpc.gw_get_transaction(...args);
      }
      return txWithStatus;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [l2tx(HexString)]
   * @returns
   */
  async execute_l2transaction(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_execute_l2transaction(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [raw_l2tx(HexString), (block_number)]
   * @returns
   */
  async execute_raw_l2transaction(
    args: [HexString, HexNumber | null | undefined]
  ) {
    // todo: check raw tx for known types(including gasless tx)
    try {
      args[1] = formatHexNumber(args[1]);

      const executeCallResult = async () => {
        let result: RunResult =
          await this.readonlyRpc.gw_execute_raw_l2transaction(...args);
        const stringifyResult = JSON.stringify(result);
        return stringifyResult;
      };

      if (envConfig.enableCacheExecuteRawL2Tx === "true") {
        // calculate raw data cache key
        const [tipBlockHash, memPoolStateRoot] = await Promise.all([
          this.readonlyRpc.gw_get_tip_block_hash(),
          this.readonlyRpc.gw_get_mem_pool_state_root(),
        ]);
        const serializeParams = serializeExecuteRawL2TxParameters(
          args[0],
          args[1]
        );
        const rawDataKey = getExecuteRawL2TxCacheKey(
          serializeParams,
          tipBlockHash,
          memPoolStateRoot
        );

        const prefixName = `${this.constructor.name}:execute_raw_l2tx`; // FIXME: ${this.call.name} is null
        const constructArgs: DataCacheConstructor = {
          prefixName,
          rawDataKey,
          executeCallResult,
        };
        const dataCache = new RedisDataCache(constructArgs);
        const stringifyResult = await dataCache.get();
        return JSON.parse(stringifyResult);
      } else {
        // not using cache
        const stringifyResult = await executeCallResult();
        return JSON.parse(stringifyResult);
      }
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [l2tx(HexString)]
   * @returns
   */
  async submit_l2transaction(args: [HexString]) {
    try {
      // validate l2 tx params
      const serializedL2Tx = args[0];
      const l2Tx = parseSerializeL2Transaction(serializedL2Tx);

      const toId: HexNumber = l2Tx.raw.to_id;
      const toScriptHash: Hash | undefined =
        await this.readonlyRpc.gw_get_script_hash(toId);
      if (toScriptHash == null) {
        throw new InvalidParamsError(
          `invalid l2Transaction, toScriptHash not found.`
        );
      }
      const toScript: Script | undefined = await this.readonlyRpc.gw_get_script(
        toScriptHash
      );
      if (toScript == null) {
        throw new InvalidParamsError(
          `invalid l2Transaction, toScript not found.`
        );
      }

      const fromId: HexNumber = l2Tx.raw.from_id;
      const fromScriptHash: Hash | undefined =
        await this.readonlyRpc.gw_get_script_hash(fromId);
      if (fromScriptHash == null) {
        throw new InvalidParamsError(
          `invalid l2Transaction, fromScriptHash not found.`
        );
      }
      const fromScript: Script | undefined =
        await this.readonlyRpc.gw_get_script(fromScriptHash);
      if (fromScript == null) {
        throw new InvalidParamsError(
          `invalid l2Transaction, fromScript not found.`
        );
      }

      const [minFeeRate, minGasPrice] = await Promise.all([
        readonlyPriceOracle.minFeeRate(),
        readonlyPriceOracle.minGasPrice(),
      ]);

      // 1. validate polyjuice tx params
      if (
        toScript.code_hash ===
          gwConfig.backends.polyjuice.validatorScriptTypeHash &&
        isPolyjuiceTransactionArgs(l2Tx.raw.args)
      ) {
        const decodeData = decodePolyjuiceArgs(l2Tx.raw.args);

        const gasLimitErr = verifyGasLimit(decodeData.gasLimit, 0);
        if (gasLimitErr) {
          throw gasLimitErr.padContext(`gw_submit_l2transaction`);
        }

        const to = decodeData.isCreate
          ? undefined
          : "0x" + toScript.args.slice(2).slice((32 + 4) * 2);

        // Check intrinsic gas and enough fund
        // Note: gasless tx will set gas price = 0, can pass intrinsic gas check as well
        let from = "0x" + fromScript.args.slice(2).slice(64, 104);
        // For auto create account tx, from address should recover from signature
        if (fromId === "0x0") {
          const r = l2Tx.signature.slice(0, 66);
          const s = "0x" + l2Tx.signature.slice(66, 130);
          const recoverId = "0x" + l2Tx.signature.slice(130, 132);
          const v = BigInt(l2Tx.raw.chain_id) * 2n + 35n + BigInt(recoverId);
          const polyjuiceTransaction: PolyjuiceTransaction = {
            nonce: l2Tx.raw.nonce,
            gasPrice: decodeData.gasPrice,
            gasLimit: decodeData.gasLimit,
            to: to || "0x",
            value: decodeData.value,
            data: decodeData.input,
            r,
            s,
            v: "0x" + v.toString(16),
          };
          from = recoverEthAddressFromPolyjuiceTx(polyjuiceTransaction);
          logger.debug("gw_submit_l2transaction recovered from:", from);
        }
        const value = decodeData.value;
        const input = decodeData.input;
        const gas = decodeData.gasLimit;
        const gasPrice = decodeData.gasPrice;

        const intrinsicGasErr = verifyIntrinsicGas(to, input, gas, 0);
        if (intrinsicGasErr) {
          throw intrinsicGasErr.padContext(`gw_submit_l2transaction`);
        }

        // only check if it is gasless transaction when entrypointContract is configured
        if (
          gwConfig.entrypointContract != null &&
          isGaslessTransaction(
            {
              to: to || "0x",
              gasPrice: gasPrice === "0x" ? "0x0" : gasPrice,
              data: input,
            },
            gwConfig.entrypointContract
          )
        ) {
          const err = verifyGaslessTransaction(
            to || "0x",
            input,
            gasPrice === "0x" ? "0x0" : gasPrice,
            gas === "0x" ? "0x0" : gas,
            0
          );
          if (err != null) {
            throw err.padContext(`gw_submit_l2transaction`);
          }
        } else {
          // not gasless transaction, check gas price
          const gasPriceErr = verifyGasPrice(
            decodeData.gasPrice,
            minGasPrice,
            0
          );
          if (gasPriceErr) {
            throw gasPriceErr.padContext(`gw_submit_l2transaction`);
          }
        }

        const client = new GodwokenClient(envConfig.godwokenJsonRpc);
        const enoughBalanceErr = await verifyEnoughBalance(
          client,
          from,
          value,
          gas,
          gasPrice,
          0
        );
        if (enoughBalanceErr) {
          throw enoughBalanceErr.padContext(`gw_submit_l2transaction`);
        }
      }

      // 2. validate SUDT transfer l2 transaction fee
      //    since fee is all pCKB, there is no need to check to sudt id
      if (toScript.code_hash === gwConfig.gwScripts.l2Sudt.typeHash) {
        const sudtArgs = parseSerializeSudtArgs(l2Tx.raw.args);
        if (sudtArgs.type === SudtArgsType.SUDTTransfer) {
          const fee = (sudtArgs.value as SudtTransfer).fee.amount;
          const feeErr = verifyL2TxFee(fee, serializedL2Tx, minFeeRate, 0);
          if (feeErr) {
            throw feeErr.padContext(`gw_submit_l2transaction`);
          }
        }
      }

      // 3. validate ethAddrReg setMapping l2 transaction fee
      if (
        toId === gwConfig.accounts.ethAddrReg.id &&
        toScript.code_hash ===
          gwConfig.backends.ethAddrReg.validatorScriptTypeHash
      ) {
        const regArgs = parseSerializeEthAddrRegArgs(l2Tx.raw.args);

        if (
          regArgs.type === EthAddrRegArgsType.SetMapping ||
          regArgs.type === EthAddrRegArgsType.BatchSetMapping
        ) {
          const fee = (regArgs.value as SetMapping | BatchSetMapping).fee
            .amount;
          const feeErr = verifyL2TxFee(fee, serializedL2Tx, minFeeRate, 0);
          if (feeErr) {
            throw feeErr.padContext(
              `gw_submit_l2transaction ethAddrReg ${regArgs.type}`
            );
          }
        }
      }

      // 4. disallow meta contract tx
      if (toId === META_CONTRACT_ID) {
        throw new Error("Meta contract transaction is disallowed");
      }

      // pass validate, submit l2 tx
      const result = await this.rpc.gw_submit_l2transaction(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [withdrawal_request(HexString)]
   * @returns
   */
  async submit_withdrawal_request(args: any[]) {
    try {
      const result = await this.rpc.gw_submit_withdrawal_request(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [scriptHash(Hash), registryId(HexNumber)]
   * @returns
   */
  async get_registry_address_by_script_hash(args: any[]) {
    try {
      const result = await this.rpc.gw_get_registry_address_by_script_hash(
        ...args
      );
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [registryAddress(HexString)]
   * @returns
   */
  async get_script_hash_by_registry_address(args: any[]) {
    try {
      const registryAddress: string = args[0];
      const key = `${GW_RPC_KEY}_addr_${registryAddress}`;
      const value = await this.gwCache.get(key);
      if (value != null) {
        logger.debug(
          `using cache : registryAddress(${registryAddress}) -> scriptHash(${value})`
        );
        return value;
      }

      const result =
        await this.readonlyRpc.gw_get_script_hash_by_registry_address(...args);
      if (result != null) {
        logger.debug(
          `update cache: registryAddress(${registryAddress}) -> scriptHash(${result})`
        );
        this.gwCache.insert(key, result);
      }
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args []
   * @returns
   */
  async get_fee_config(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_fee_config(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  /**
   *
   * @param args [withdraw_tx_hash]
   * @returns
   */
  async get_withdrawal(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_withdrawal(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async get_last_submitted_info(args: any[]) {
    try {
      const result = await this.rpc.gw_get_last_submitted_info(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async get_node_info(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_node_info(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async is_request_in_queue(args: any[]) {
    try {
      const result = await this.rpc.gw_is_request_in_queue(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async get_pending_tx_hashes(args: any[]) {
    try {
      const result = await this.readonlyRpc.gw_get_pending_tx_hashes(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }

  async debug_replay_transaction(args: []) {
    try {
      const result = await this.readonlyRpc.debug_replay_transaction(...args);
      return result;
    } catch (error) {
      handleGwError(error);
    }
  }
}

function formatHexNumber(
  num: HexNumber | undefined | null
): HexNumber | undefined | null {
  if (num == null) {
    return num;
  }

  return num.toLowerCase();
}

function serializeExecuteRawL2TxParameters(
  serializeRawL2Tx: HexString,
  blockNumber: HexNumber | null | undefined
): HexString {
  const toSerializeObj = {
    serializeRawL2Tx,
    blockNumber: blockNumber || "0x",
  };
  return JSON.stringify(toSerializeObj);
}

function getExecuteRawL2TxCacheKey(
  serializeParameter: string,
  tipBlockHash: HexString,
  memPoolStateRoot: HexString
) {
  const hash =
    "0x" + utils.ckbHash(Buffer.from(serializeParameter)).serializeJson();
  const cacheKey = `0x${tipBlockHash.slice(2, 18)}${memPoolStateRoot.slice(
    2,
    18
  )}${hash.slice(2, 18)}`;
  return cacheKey;
}
