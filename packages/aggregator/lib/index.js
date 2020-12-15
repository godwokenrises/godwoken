const addon = require("../native");

const fs = require("fs");

const { TransactionCollector, CellCollector } = require("@ckb-lumos/indexer");

function defaultLogger(level, message) {
  console.log(`[${level}] ${message}`);
}

class Chain {
  construct(
    configPath,
    indexer,
    {
      pollIntervalSeconds = 2,
      livenessCheckIntervalSeconds = 5,
      logger = defaultLogger,
    } = {}
  ) {
    this.indexer = indexer;
    this.livenessCheckIntervalSeconds = livenessCheckIntervalSeconds;
    this.logger = logger;
    this.config = parseConfig(configPath);
    this.nativeChain = new addon.NativeChain(configPath);
  }

  start() {
      setInterval(() => {
          this.sync();
      }, this.pollIntervalSeconds * 1000);
  }

  startForever() {
    this.start();
    setInterval(() => {
      if (!this.nativeChain.running()) {
        this.logger("error", "Native chain has stopped, maybe check the log?");
        this.start();
      }
    }, this.livenessCheckIntervalSeconds * 1000);
  }

  sync() {
    // start from last_synced block
    let updates = [];
    let reverts = [];
    const fromBlock = this.nativeChain.last_synced();
    const depositionQueryOptions = {
        lock: {
            script: {
                code_hash: this.config.chain.deposition_lock_script,
                hash_type: "type",
                args: "any",
            },
            ioType: "output",
        },
        fromBlock: "0x" + BigInt(fromBlock).toString(16),
    };
    const depositionTransactionCollector = new TransactionCollector(depositionQueryOptions);
    for await (const tx of depositionTransactionCollector.collect()) {
        if (tx.tx_status.status != "committed") {
            continue;
        }
        const block_hash = tx.tx_status.block_hash;
        const transactionInfo = {
            transaction: tx.transaction,
            block_hash: block_hash,
        };
        const block = await this.indexer.rpc.get_block(block_hash);
        const number = block.header.number;
        const headerInfo = {
            number: number,
            block_hash: block_hash,
        };
        let depositionRequests= [];
        for (const index of tx.transaction.outputs.length) {
            const cell = tx.transaction.outputs[index];
            const data = tx.transaction.outputs_data[index];
            if (cell.code_hash === this.config.chain.deposition_lock_script && cell.hash_type === "type") {
                const decodedArgs = decodeDepositionLockScriptArgs(cell.args);
                const capacity = cell.capacity;
                const layer2_ckb_script = {
                    code_hash: "",
                    hash_type: "",
                    args: "",
                };
                const ckbDepositionRequest = {
                    script: decodedArgs.layer2_lock,
                    sudt_script: layer2_ckb_script,
                    value: capacity,
                };
                depositionRequests.push(ckbDepositionRequest);
                if (cell.type) {
                    const depositionRequest = {
                        script: decodedArgs.layer2_lock,
                        sudt_script: cell.type,
                        value: data,
                    };
                    depositionRequests.push(sudtDepositionRequest);
                }
            } else {
                continue;
            }
        }
        const l1ActionContext = {
            deposition_requests: depositionRequests,
            // TODO
            withdrawal_requests: [],
        };
        const l1Action = {
            transaction_info: transactionInfo,
            header_info: headerInfo,
            context: l1ActionContext,
        };
        //TODO serialize syncTransition
        updates.push(l1Action);
    }

      const depositionToCustodianQueryOptions = {
          lock: {
              script: {
                  code_hash: this.config.chain.rollup_custodian_lock,
                  hash_type: "type",
                  args: "any",
              },
              ioType: "output",
          }
          fromBlock: "0x" + BigInt(fromBlock).toString(16),
      };
      const depositionToCustodianTransactionCollector = new TransactionCollector(depositionToCustodianQueryOptions);
      for await(const tx of depositionToCustodianTransactionCollector.collect()) {

      }
    //TODO reverts
    const nextBlockContext = {
        aggregator_id: 0,
        timestamp: 1,
    };
    this.nativeChain.sync(updates, reverts, nextBlockContext);
  }

  produce_block(aggregator_id, deposition_requests, withdrawal_requests) {}
}

function parseConfig(configPath) {
    let rawData = fs.readFileSync(configPath);
    return JSON.parse(rawData);
}
funcion decodeDepositionLockScriptArgs(args) {
    // TODO decode
    const decodedArgs = {
        rollup_type_script: rollupTypeScript,
        layer2_lock: layer2Lock,
        owner_lock_hash: ownerLockHash,
        cancel_timeout: cancel_timeout,
    };
    return decodedArgs;
}
module.exports = { Chain };
