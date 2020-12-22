import { normalizers, Reader, RPC } from "ckb-js-toolkit";
import {
  core,
  utils,
  Cell,
  Hash,
  HexNumber,
  HexString,
  Transaction,
  TransactionWithStatus,
  QueryOptions,
} from "@ckb-lumos/base";
import {
  Indexer,
  CellCollector,
  TransactionCollector,
} from "@ckb-lumos/indexer";
import { Config, ChainService, SubmitTxs } from "@ckb-godwoken/godwoken";
import { DeploymentConfig } from "./config";
import {
  L2Block,
  SerializeHeaderInfo,
  SerializeCustodianLockArgs,
} from "../schemas/godwoken";
import {
  DepositionRequest,
  HeaderInfo,
  NormalizeHeaderInfo,
  NormalizeCustodianLockArgs,
} from "./types";
import {
  DepositionEntry,
  scanDepositionCellsInCommittedL2Block,
  tryExtractDepositionRequest,
} from "./utils";

function asyncSleep(ms = 0) {
  return new Promise((r) => setTimeout(r, ms));
}

function isRollupTransction(
  tx: Transaction,
  config: DeploymentConfig
): boolean {
  for (const output of tx.outputs) {
    const { lock } = output;
    if (
      lock.code_hash === config.state_validator_type.code_hash &&
      lock.hash_type === config.state_validator_type.hash_type &&
      lock.args.length >= 66 &&
      lock.args.substr(0, 66) === config.rollup_type_hash
    ) {
      return true;
    }
  }
  return false;
}

export class Runner {
  rpc: RPC;
  indexer: Indexer;
  chainService: ChainService;
  deploymentConfig: DeploymentConfig;
  lastBlockNumber: bigint;
  cancelListener: () => void;

  // TODO: change to PoA
  lastProduceBlockTime: bigint;

  constructor(
    rpc: RPC,
    indexer: Indexer,
    chainService: ChainService,
    deploymentConfig: DeploymentConfig
  ) {
    this.rpc = rpc;
    this.indexer = indexer;
    this.chainService = chainService;
    this.deploymentConfig = deploymentConfig;
    this.lastBlockNumber = 0n;
    this.cancelListener = () => {};
    this.lastProduceBlockTime = 0n;
  }

  _rollupCellQueryOptions(): QueryOptions {
    return {
      type: {
        script: {
          code_hash: this.deploymentConfig.state_validator_type.code_hash,
          hash_type: this.deploymentConfig.state_validator_type.hash_type,
          args: this.deploymentConfig.rollup_type_hash,
        },
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
          code_hash: this.deploymentConfig.deposition_lock.code_hash,
          hash_type: this.deploymentConfig.deposition_lock.hash_type,
          args: this.deploymentConfig.rollup_type_hash,
        },
        ioType: "output",
        argsLen: "any",
      },
      order: "asc",
    };
  }

  async _syncL2Block(transaction: Transaction, headerInfo: HeaderInfo) {
    const depositionRequests = await scanDepositionCellsInCommittedL2Block(
      transaction,
      this.deploymentConfig,
      this.rpc
    );
    const context: SubmitTxs = {
      type: "submit_txs",
      deposition_requests: depositionRequests,
    };
    const update = {
      transaction: core.SerializeTransaction(
        normalizers.NormalizeTransaction(transaction)
      ),
      header_info: SerializeHeaderInfo(NormalizeHeaderInfo(headerInfo)),
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
  }

  async _syncToTip() {
    while (true) {
      const blockNumber = this.lastBlockNumber + 1n;
      const block = await this.rpc.get_block_by_number(
        "0x" + blockNumber.toString(16)
      );
      if (!block) {
        // Already synced to tip
        return;
      }
      const headerInfo: HeaderInfo = {
        number: block.header.number,
        block_hash: block.header.hash,
      };
      for (const tx of block.transactions) {
        if (isRollupTransction(tx, this.deploymentConfig)) {
          await this._syncL2Block(tx, headerInfo);
        }
      }
      this.lastBlockNumber = BigInt(headerInfo.number);
    }
  }

  async start() {
    // Wait for indexer sync
    await this.indexer.waitForSync();

    // Use rollup type hash to look for transactions in a backwards fashion, using those
    // transactions, we can then rebuild godwoken internal states. In a future version
    // where godwoken has persistent storage, things could be simplified, but right now
    // this provides a good solution for cold start.
    // TODO: for now, we always rebuild state from genesis cell, so there won't be a
    // forking problem. However if later we implement persistent state store, forking
    // will then need to be handled here.
    const committedL2BlockCollector = new TransactionCollector(
      this.indexer,
      this._rollupCellQueryOptions(),
      {
        includeStatus: true,
      }
    );
    // TODO: confirm how genesis will be handled later. If genesis is handled
    // in chain service initialization, we need to skip one transaction here.
    for await (const result of committedL2BlockCollector.collect()) {
      const txWithStatus = result as TransactionWithStatus;
      const transaction: Transaction = txWithStatus.transaction;
      const blockHash: Hash = txWithStatus.tx_status.block_hash!;
      const header = await this.rpc.get_header(blockHash);
      const headerInfo = {
        number: header.number,
        block_hash: header.hash,
      };
      await this._syncL2Block(transaction, headerInfo);
      this.lastBlockNumber = BigInt(headerInfo.number);
    }

    // Due to on-going syncing, previous methods might result in some blocks not being
    // picked up. Here, we will need to first play catch-up work, till we reached tip
    // version.
    await this._syncToTip();

    // Now we can boot godwoken to a normal working state: we listen for each new block,
    // look for godwoken state changes, which we need to send to the internal godwoken
    // state machine. Each new block will also incur new timestamp change. At certain
    // time, we need to decide to issue a new L2 block.

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
    const collector = new CellCollector(
      this.indexer,
      this._depositionCellQueryOptions()
    );
    const results = [];
    for await (const cell of collector.collect()) {
      const cellHeader = await this.rpc.get_header(cell.block_hash);
      const entry = await tryExtractDepositionRequest(
        cell,
        this.deploymentConfig,
        tipHeader,
        cellHeader
      );
      if (entry) {
        results.push(entry);
        if (results.length === maximum) {
          break;
        }
      }
    }
    return results;
  }

  async _queryLiveRollupCell(): Promise<Cell> {
    const collector = new CellCollector(
      this.indexer,
      this._rollupCellQueryOptions()
    );
    const results = [];
    for await (const cell of collector.collect()) {
      results.push(cell);
    }
    if (results.length !== 1) {
      throw new Error("Invalid number of rollup cells!");
    }
    return results[0];
  }

  _generateCustodianCells(
    packedl2Block: ArrayBuffer,
    depositionEntries: DepositionEntry[]
  ) {
    const l2Block = new L2Block(packedl2Block);
    const rawL2Block = l2Block.getRaw();
    const data: DataView = (rawL2Block as any).view;
    const l2BlockHash = utils.ckbHash(data.buffer).serializeJson();
    const l2BlockNumber =
      "0x" + rawL2Block.getNumber().toLittleEndianUint64().toString(16);
    const custodianCells = depositionEntries.map(({ cell, lockArgs }) => {
      const custodianLockArgs = {
        owner_lock_hash: new Reader(
          lockArgs.getOwnerLockHash().raw()
        ).serializeJson(),
        deposition_block_hash: l2BlockHash,
        deposition_block_number: l2BlockNumber,
      };
      const packedCustodianLockArgs = SerializeCustodianLockArgs(
        NormalizeCustodianLockArgs(custodianLockArgs)
      );
      const buffer = new ArrayBuffer(32 + packedCustodianLockArgs.byteLength);
      const array = new Uint8Array(buffer);
      array.set(
        new Uint8Array(
          new Reader(this.deploymentConfig.rollup_type_hash).toArrayBuffer()
        ),
        0
      );
      array.set(new Uint8Array(packedCustodianLockArgs), 32);
      const lock = {
        code_hash: this.deploymentConfig.custodian_lock.code_hash,
        hash_type: this.deploymentConfig.custodian_lock.hash_type,
        args: new Reader(buffer).serializeJson(),
      };
      return {
        capacity: cell.cell_output.capacity,
        lock,
        type: cell.cell_output.type,
      };
    });
    const custodianData: HexString[] = depositionEntries.map(
      ({ cell }) => cell.data
    );

    return {
      cells: custodianCells,
      data: custodianData,
    };
  }

  _newBlockReceived(medianTimeHex: HexNumber) {
    (async () => {
      await this._syncToTip();
      const medianTime = BigInt(medianTimeHex);
      // TODO: change to PoA later, right now it issues a new L2 block every 20 seconds.
      if (medianTime - this.lastProduceBlockTime >= 20n * 1000n) {
        const depositionEntries = await this._queryValidDepositionRequests();
        const depositionRequests = depositionEntries.map(
          ({ packedRequest }) => packedRequest
        );
        const depositionInputs = depositionEntries.map(({ cell }) => {
          return {
            previous_output: cell.out_point!,
            since: "0x0",
          };
        });
        const param = {
          aggregator_id: "0x0",
          deposition_requests: depositionRequests,
        };
        const {
          block: packedl2Block,
          global_state,
        } = await this.chainService.produce_block(param);

        const {
          cells: custodianCells,
          data: custodianData,
        } = this._generateCustodianCells(packedl2Block, depositionEntries);
        const cell = await this._queryLiveRollupCell();
        const tx: Transaction = {
          version: "0x0",
          // TODO: fill in cell deps
          cell_deps: [],
          header_deps: [],
          // TODO: fill in withdrawed custodian cells
          inputs: [
            {
              previous_output: cell.out_point!,
              // TODO: PoA might need this
              since: "0x0",
            },
          ].concat(depositionInputs),
          // TODO: fill in created withdraw cells
          outputs: [cell.cell_output].concat(custodianCells),
          outputs_data: [new Reader(global_state).serializeJson()].concat(
            custodianData
          ),
          witnesses: [new Reader(packedl2Block).serializeJson()],
        };
        const hash = await this.rpc.send_transaction(tx);
        console.log(`Submitted l2 block in ${hash}`);
        this.lastProduceBlockTime = medianTime;
      }
    })().catch((e) => {
      console.error(`Error processing new block: ${e}`);
      // Clear the median time subscriber to exit
      this.cancelListener();
    });
  }
}
