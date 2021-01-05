import { normalizers, Reader, RPC } from "ckb-js-toolkit";
import {
  core,
  utils,
  values,
  Cell,
  CellDep,
  Hash,
  HexNumber,
  HexString,
  Script,
  Transaction,
  TransactionWithStatus,
  QueryOptions,
  Indexer,
} from "@ckb-lumos/base";
import { common } from "@ckb-lumos/common-scripts";
import { getConfig } from "@ckb-lumos/config-manager";
import {
  TransactionSkeleton,
  sealTransaction,
  scriptToAddress,
} from "@ckb-lumos/helpers";
import { ChainService, SubmitTxs } from "@ckb-godwoken/godwoken";
import {
  asyncSleep,
  waitForBlockSync,
  DeploymentConfig,
  schemas,
  types,
} from "@ckb-godwoken/base";
import {
  DepositionEntry,
  scanDepositionCellsInCommittedL2Block,
  tryExtractDepositionRequest,
  RunnerConfig,
  StateValidatorLockGenerator,
  Logger,
} from "./utils";
import { AlwaysSuccessGenerator } from "./locks";
import { generator as poaGeneratorModule } from "clerkb-lumos-integrator";
import * as secp256k1 from "secp256k1";
import { exit } from "process";

function isRollupTransction(
  tx: Transaction,
  rollupTypeScript: Script
): boolean {
  const rollupValue = new values.ScriptValue(rollupTypeScript, {
    validate: false,
  });
  for (const output of tx.outputs) {
    if (output.type) {
      const value = new values.ScriptValue(output.type, { validate: false });
      if (value.equals(rollupValue)) {
        return true;
      }
    }
  }
  return false;
}

export class Runner {
  rpc: RPC;
  indexer: Indexer;
  chainService: ChainService;
  config: RunnerConfig;
  lastBlockNumber: bigint;
  cancelListener: () => void;
  logger: Logger;
  rollupTypeHash: Hash;
  privateKey: HexString;

  lockGenerator?: StateValidatorLockGenerator;

  constructor(
    rpc: RPC,
    indexer: Indexer,
    chainService: ChainService,
    config: RunnerConfig,
    privateKey: HexString,
    logger: Logger
  ) {
    this.rpc = rpc;
    this.indexer = indexer;
    this.chainService = chainService;
    this.config = config;
    this.cancelListener = () => {};
    this.logger = logger;
    this.privateKey = privateKey;
    this.rollupTypeHash = utils
      .ckbHash(
        core.SerializeScript(
          normalizers.NormalizeScript(
            config.godwokenConfig.chain.rollup_type_script
          )
        )
      )
      .serializeJson();

    const lastSynced = new schemas.HeaderInfo(
      new Reader(chainService.lastSynced()).toArrayBuffer()
    );
    this.lastBlockNumber = lastSynced.getNumber().toLittleEndianBigUint64();

    if (!this._readOnlyMode()) {
      if (config.aggregatorConfig.type === "poa") {
        this.lockGenerator = new poaGeneratorModule.PoAGenerator(
          this._ckbAddress(),
          this.indexer,
          [config.deploymentConfig.poa_state_dep!],
          (message) => {
            this.logger("debug", `[aggregator] ${message}`);
          }
        );
      } else {
        this.lockGenerator = new AlwaysSuccessGenerator();
      }
    }
  }

  _readOnlyMode(): boolean {
    return !this.privateKey;
  }

  _ckbAddress(): string {
    if (this._readOnlyMode()) {
      throw new Error("Read only mode is used!");
    }
    const privateKeyBuffer = new Reader(this.privateKey).toArrayBuffer();
    const publicKeyArray = secp256k1.publicKeyCreate(
      new Uint8Array(privateKeyBuffer)
    );
    const publicKeyHash = utils
      .ckbHash(publicKeyArray.buffer)
      .serializeJson()
      .substr(0, 42);
    const scriptConfig = getConfig().SCRIPTS.SECP256K1_BLAKE160!;
    const script = {
      code_hash: scriptConfig.CODE_HASH,
      hash_type: scriptConfig.HASH_TYPE,
      args: publicKeyHash,
    };
    return scriptToAddress(script);
  }

  _deploymentConfig(): DeploymentConfig {
    return this.config.deploymentConfig;
  }

  _rollupCellQueryOptions(): QueryOptions {
    return {
      type: {
        script: this.config.godwokenConfig.chain.rollup_type_script,
        ioType: "output",
        argsLen: "any",
      },
      // TODO: when persistent store is built, we can add fromBlock here.
      order: "asc",
    };
  }

  _depositionCellQueryOptions(): QueryOptions {
    return {
      lock: {
        script: {
          code_hash: this._deploymentConfig().deposition_lock.code_hash,
          hash_type: this._deploymentConfig().deposition_lock.hash_type,
          args: this.rollupTypeHash,
        },
        ioType: "output",
        argsLen: "any",
      },
      order: "asc",
    };
  }

  async _syncL2Block(transaction: Transaction, headerInfo: types.HeaderInfo) {
    const depositionRequests = await scanDepositionCellsInCommittedL2Block(
      transaction,
      this.config,
      this.rpc
    );
    const context: SubmitTxs = {
      type: "submit_txs",
      deposition_requests: depositionRequests,
    };
    const update = {
      transaction: new Reader(
        core.SerializeTransaction(normalizers.NormalizeTransaction(transaction))
      ).serializeJson(),
      header_info: new Reader(
        schemas.SerializeHeaderInfo(types.NormalizeHeaderInfo(headerInfo))
      ).serializeJson(),
      context,
    };
    const syncParam = {
      reverts: [],
      updates: [update],
      // TODO: figure out next block context values
      next_block_context: {
        aggregator_id: "0x0",
        timestamp: "0x" + (BigInt(Date.now()) / 1000n).toString(16),
      },
    };
    // TODO: process sync event.
    const event = await this.chainService.sync(syncParam);
    this.logger(
      "info",
      `Synced l2 blocks at l1 block number: ${headerInfo.number}`
    );
  }

  async _syncToTip() {
    while (true) {
      const blockNumber = this.lastBlockNumber + 1n;
      const block = await this.rpc.get_block_by_number(
        "0x" + blockNumber.toString(16)
      );
      if (!block) {
        // Already synced to tip
        await waitForBlockSync(this.indexer, this.rpc, undefined, blockNumber);
        return;
      }
      const headerInfo: types.HeaderInfo = {
        number: block.header.number,
        block_hash: block.header.hash,
      };
      for (const tx of block.transactions) {
        if (
          isRollupTransction(
            tx,
            this.config.godwokenConfig.chain.rollup_type_script
          )
        ) {
          await this._syncL2Block(tx, headerInfo);
        }
      }
      this.lastBlockNumber = BigInt(headerInfo.number);
    }
  }

  async start() {
    this.logger("debug", `Rollup Type Hash: ${this.rollupTypeHash}`);
    if (this._readOnlyMode()) {
      this.logger("info", "Current server is running in readonly mode!");
    } else {
      this.logger("debug", `CKB Address: ${this._ckbAddress()}`);
    }
    // Wait for indexer sync
    await this.indexer.waitForSync();

    this.logger("info", "Catching up to tip!");
    await this._syncToTip();

    // Now we can boot godwoken to a normal working state: we listen for each new block,
    // look for godwoken state changes, which we need to send to the internal godwoken
    // state machine. Each new block will also incur new timestamp change. At certain
    // time, we need to decide to issue a new L2 block.

    this.logger("info", "Subscribe to median time!");
    const callback = this._newBlockReceived.bind(this);
    const medianTimeEmitter = this.indexer.subscribeMedianTime();
    medianTimeEmitter.on("changed", callback);
    this.cancelListener = () => {
      medianTimeEmitter.off("changed", callback);
    };
  }

  async _queryValidDepositionRequests(
    maximum = 20
  ): Promise<Array<DepositionEntry>> {
    const tipHeader = await this.rpc.get_tip_header();
    const collector = this.indexer.collector(
      this._depositionCellQueryOptions()
    );
    const results = [];
    for await (const cell of collector.collect()) {
      // Since custodian cells requires much bigger storage, ignore
      // deposition request with less than 400 CKB for now.
      if (BigInt(cell.cell_output.capacity) < 40000000000n) {
        continue;
      }
      const cellHeader = await this.rpc.get_header(cell.block_hash);
      try {
        const entry = await tryExtractDepositionRequest(
          cell,
          this.config,
          tipHeader,
          cellHeader
        );
        if (entry) {
          results.push(entry);
          if (results.length === maximum) {
            break;
          }
        }
      } catch (e) {
        this.logger(
          "error",
          `Ignoring deposition cell ${cell.out_point!.tx_hash} - ${
            cell.out_point!.index
          } error: ${e}`
        );
      }
    }
    return results;
  }

  // Use the transaction containing specified cell to look for a dep cell
  // one can use here.
  // TODO: this works but is quite slow, maybe we can cache found cell deps
  // to reduce queries to CKB.
  async _queryTypeScriptCellDep(cell: Cell): Promise<CellDep> {
    const tx: Transaction = (
      await this.rpc.get_transaction(cell.out_point!.tx_hash)
    ).transaction;
    for (const cellDep of tx.cell_deps) {
      if (cellDep.dep_type === "dep_group") {
        // throw new Error("TODO: dep group support!");
        continue;
      }
      const codeCell = await this.rpc.get_live_cell(cellDep.out_point, true);
      if (cell.cell_output.type!.hash_type == "data") {
        if (codeCell.cell.data.hash === cell.cell_output.type!.code_hash) {
          return cellDep;
        }
      } else if (codeCell.cell.output.type) {
        const typeHash = utils.computeScriptHash(codeCell.cell.output.type);
        if (typeHash === cell.cell_output.type!.code_hash) {
          return cellDep;
        }
      }
    }
    throw new Error(`Cannot find cell dep for ${cell.cell_output.type!}`);
  }

  async _queryLiveRollupCell(): Promise<Cell> {
    const collector = this.indexer.collector(this._rollupCellQueryOptions());
    const results = [];
    for await (const cell of collector.collect()) {
      results.push(cell);
    }
    if (results.length !== 1) {
      throw new Error(`Invalid number of rollup cells: ${results.length}`);
    }
    return results[0];
  }

  _generateCustodianCells(
    packedl2Block: HexString,
    depositionEntries: DepositionEntry[]
  ) {
    const l2Block = new schemas.L2Block(
      new Reader(packedl2Block).toArrayBuffer()
    );
    const rawL2Block = l2Block.getRaw();
    const data: DataView = (rawL2Block as any).view;
    const l2BlockHash = utils.ckbHash(data.buffer).serializeJson();
    const l2BlockNumber =
      "0x" + rawL2Block.getNumber().toLittleEndianBigUint64().toString(16);
    return depositionEntries.map(({ cell, lockArgs }) => {
      const custodianLockArgs = {
        deposition_lock_args: types.DenormalizeDepositionLockArgs(lockArgs),
        deposition_block_hash: l2BlockHash,
        deposition_block_number: l2BlockNumber,
      };
      const packedCustodianLockArgs = schemas.SerializeCustodianLockArgs(
        types.NormalizeCustodianLockArgs(custodianLockArgs)
      );
      const buffer = new ArrayBuffer(32 + packedCustodianLockArgs.byteLength);
      const array = new Uint8Array(buffer);
      array.set(
        new Uint8Array(new Reader(this.rollupTypeHash).toArrayBuffer()),
        0
      );
      array.set(new Uint8Array(packedCustodianLockArgs), 32);
      const lock = {
        code_hash: this._deploymentConfig().custodian_lock.code_hash,
        hash_type: this._deploymentConfig().custodian_lock.hash_type,
        args: new Reader(buffer).serializeJson(),
      };
      return {
        cell_output: {
          capacity: cell.cell_output.capacity,
          lock,
          type: cell.cell_output.type,
        },
        data: cell.data,
      };
    });
  }

  _newBlockReceived(medianTimeHex: HexNumber) {
    (async () => {
      this.logger(
        "info",
        `New block received! Median time: ${medianTimeHex}(${BigInt(
          medianTimeHex
        )})`
      );
      await this._syncToTip();
      const tipCell = await this._queryLiveRollupCell();
      if (
        !this._readOnlyMode() &&
        (await this.lockGenerator!.shouldIssueNewBlock(
          medianTimeHex,
          tipCell
        )) === "Yes"
      ) {
        this.logger("info", "Generating new block!");
        const depositionEntries = await this._queryValidDepositionRequests();
        this.logger(
          "debug",
          `Valid deposition entries: ${depositionEntries.length}`
        );
        const depositionRequests = depositionEntries.map(
          ({ packedRequest }) => packedRequest
        );
        const param = {
          aggregator_id: "0x0",
          deposition_requests: depositionRequests,
        };
        const {
          block: packedl2Block,
          global_state,
        } = await this.chainService.produceBlock(param);
        const cell = await this._queryLiveRollupCell();

        let txSkeleton = TransactionSkeleton({ cellProvider: this.indexer });
        txSkeleton = txSkeleton.update("cellDeps", (cellDeps) => {
          cellDeps = cellDeps
            .push(this._deploymentConfig().state_validator_lock_dep)
            .push(this._deploymentConfig().state_validator_type_dep);
          if (depositionEntries.length > 0) {
            cellDeps = cellDeps.push(
              this._deploymentConfig().deposition_lock_dep
            );
          }
          return cellDeps;
        });
        txSkeleton = txSkeleton.update("inputs", (inputs) => inputs.push(cell));
        txSkeleton = txSkeleton.update("witnesses", (witnesses) => {
          const witnessArgs = {
            output_type: new Reader(packedl2Block).serializeJson(),
          };
          const packedWitnessArgs = new Reader(
            core.SerializeWitnessArgs(
              normalizers.NormalizeWitnessArgs(witnessArgs)
            )
          ).serializeJson();
          return witnesses.push(packedWitnessArgs);
        });
        txSkeleton = txSkeleton.update("outputs", (outputs) => {
          return outputs.push({
            cell_output: cell.cell_output,
            data: new Reader(global_state).serializeJson(),
          });
        });
        const addedCellDeps = new Set();
        for (const { cell } of depositionEntries) {
          txSkeleton = txSkeleton.update("inputs", (inputs) =>
            inputs.push(cell)
          );
          // Placeholders so we can make sure cells used to pay fees have signature
          // at correct place.
          txSkeleton = txSkeleton.update("witnesses", (witnesses) =>
            witnesses.push("0x")
          );
          // Some deposition cells might have type scripts for sUDTs, handle cell deps
          // here.
          if (cell.cell_output.type) {
            const cellDep = await this._queryTypeScriptCellDep(cell);
            const packedCellDep = new Reader(
              core.SerializeCellDep(normalizers.NormalizeCellDep(cellDep))
            ).serializeJson();
            if (!addedCellDeps.has(packedCellDep)) {
              txSkeleton = txSkeleton.update("cellDeps", (cellDeps) =>
                cellDeps.push(cellDep)
              );
              addedCellDeps.add(packedCellDep);
            }
          }
        }
        for (const cell of this._generateCustodianCells(
          packedl2Block,
          depositionEntries
        )) {
          txSkeleton = txSkeleton.update("outputs", (outputs) =>
            outputs.push(cell)
          );
        }
        txSkeleton = await this.lockGenerator!.fixTransactionSkeleton(
          medianTimeHex,
          txSkeleton
        );
        // TODO: stake cell
        // TODO: fill in withdrawed custodian cells
        // TODO: fill in created withdraw cells

        txSkeleton = await common.payFeeByFeeRate(
          txSkeleton,
          [this._ckbAddress()],
          BigInt(1000)
        );
        txSkeleton = common.prepareSigningEntries(txSkeleton);
        const signatures = [];
        for (const { message } of txSkeleton.get("signingEntries").toArray()) {
          const signObject = secp256k1.ecdsaSign(
            new Uint8Array(new Reader(message).toArrayBuffer()),
            new Uint8Array(new Reader(this.privateKey).toArrayBuffer())
          );
          const signatureBuffer = new ArrayBuffer(65);
          const signatureArray = new Uint8Array(signatureBuffer);
          signatureArray.set(signObject.signature, 0);
          signatureArray.set([signObject.recid], 64);
          const signature = new Reader(signatureBuffer).serializeJson();
          signatures.push(signature);
        }
        const tx = sealTransaction(txSkeleton, signatures);

        try {
          const hash = await this.rpc.send_transaction(tx);
          this.logger("info", `Submitted l2 block in ${hash}`);
        } catch (e) {
          this.logger("error", `Error submiting block: ${e}`);
          this.lockGenerator!.cancelIssueBlock().catch((e) => {
            console.error(`Error cancelling block: ${e}`);
          });
        }
      }
    })().catch((e) => {
      console.error(`Error processing new block: ${e} ${e.stack}`);
      exit(1);
    });
  }
}
