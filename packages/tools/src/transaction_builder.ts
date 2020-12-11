import {
  utils,
  Hash,
  Address,
  Cell,
  Script,
  Header,
  CellCollector as CellCollectorInterface,
  CellProvider,
  values,
} from "@ckb-lumos/base";
import { FromInfo, parseFromInfo } from "@ckb-lumos/common-scripts";
import {
  parseAddress,
  minimalCellCapacity,
  TransactionSkeletonType,
  Options,
  generateAddress,
  sealTransaction,
} from "@ckb-lumos/helpers";
import { Set, List } from "immutable";
import common from "@ckb-lumos/common-scripts/lib/common";
import { addCellDep } from "@ckb-lumos/common-scripts/lib/helper";
const { toBigUInt128LE, readBigUInt128LE, computeScriptHash } = utils;
import { getConfig } from "@ckb-lumos/config-manager";

interface SUDT {
  tokenId: Hash;
  amount: bigint;
}
interface DepositionRequest {
  script: Script;
  sudt_script: Script;
  amount: bigint;
  ownerLockHash: Hash;
}
interface WithdrawalRequest {
  lock_hash: Hash;
  sudt_script_hash: Hash;
  amount: bigint;
  accountScriptHash: Hash;
}
interface GlobalState {
  block_smt: Hash;
  account_smt: Hash;
  status: RollupStatus;
}
enum RollupStatus {
  Running = 0,
  Halting = 1,
}
const L2_CKB_SCRIPT: Script = {
  code_hash:
    "0x0000000000000000000000000000000000000000000000000000000000000000",
  hash_type: "type",
  args: "0x",
};
/**
 * User deposit L1Asset from CKB network to Rollup network
 * Currently only support one kind layer1 asset(either CKB or SUDT) once in a transaction
 *
 * @param txSkeleton
 * @param fromInfos
 * @param toAddress
 * @param ownerLockHash
 * @param cancelTimeout
 * @param capacity
 * @param sudt
 * @param changeAddress
 * @param tipHeader
 * @param options
 */
export async function deposit(
  txSkeleton: TransactionSkeletonType,
  fromInfos: FromInfo[],
  toAddress: Address,
  ownerLockHash: Hash,
  cancelTimeout: bigint,
  capacity?: bigint,
  sudt?: SUDT,
  changeAddress?: Address,
  //tipHeader?: Header,
  { config = undefined }: Options = {}
): Promise<TransactionSkeletonType> {
  if (!capacity && !sudt) {
    throw new Error("Either capacity or sudt must be set.");
  }
  const cellProvider = txSkeleton.get("cellProvider");
  if (!cellProvider) {
    throw new Error("Cell provider is missing!");
  }
  config = config || getConfig();
  const ROLLUP_DEPOSITION_LOCK = config.SCRIPTS.ROLLUP_DEPOSITION_LOCK!;

  // update txSkeleton's cell_dep: add deposition_lock_script dep
  txSkeleton = addCellDep(txSkeleton, {
    out_point: {
      tx_hash: ROLLUP_DEPOSITION_LOCK.TX_HASH,
      index: ROLLUP_DEPOSITION_LOCK.INDEX,
    },
    dep_type: ROLLUP_DEPOSITION_LOCK.DEP_TYPE,
  });

  // build target output cells

  let depositionCapacity = "0x0";
  let sudtTypeScript: Script | undefined = undefined;
  let data = "0x";

  if (capacity) {
    depositionCapacity = "0x" + capacity.toString(16);
  }
  if (sudt) {
    const SUDT_SCRIPT = config.SCRIPTS.SUDT!;
    // build sudt type script
    sudtTypeScript = {
      code_hash: SUDT_SCRIPT.CODE_HASH,
      hash_type: SUDT_SCRIPT.HASH_TYPE,
      args: sudt.tokenId,
    };
    data = toBigUInt128LE(sudt.amount);
    // update txSkeleton's cell_dep: add sudt_type_script dep
    txSkeleton = addCellDep(txSkeleton, {
      out_point: {
        tx_hash: SUDT_SCRIPT.TX_HASH,
        index: SUDT_SCRIPT.INDEX,
      },
      dep_type: SUDT_SCRIPT.DEP_TYPE,
    });
  }

  // build deposition lock script
  const ROLLUP_TYPE_SCRIPT = config.SCRIPTS.ROLLUP_TYPE_SCRIPT!;
  const rollupTypeScript: Script = {
    code_hash: ROLLUP_TYPE_SCRIPT.CODE_HASH,
    hash_type: ROLLUP_TYPE_SCRIPT.HASH_TYPE,
    args: "0x", //TODO reset the args
  };
  const rollupTypeHash: Hash = computeScriptHash(rollupTypeScript);
  const toL2LockScript: Script = parseAddress(toAddress, { config });
  const args = encodeDepositionLockArgs(
    rollupTypeHash,
    toL2LockScript,
    ownerLockHash,
    cancelTimeout
  );
  const depositionLockScript: Script = {
    code_hash: ROLLUP_DEPOSITION_LOCK.CODE_HASH,
    hash_type: ROLLUP_DEPOSITION_LOCK.HASH_TYPE,
    args: args,
  };
  // assemble deposition cell
  const targetOutput: Cell = {
    cell_output: {
      capacity: depositionCapacity,
      lock: depositionLockScript,
      type: sudtTypeScript,
    },
    data: data,
    out_point: undefined,
    block_hash: undefined,
  };
  // update deposition cell's capacity if the capacity parameter is undefined
  // or less than minimalCellCapacity
  const minimalCapacity = minimalCellCapacity(targetOutput);
  if (!capacity || capacity < minimalCapacity) {
    targetOutput.cell_output.capacity =
      "0x" + BigInt(minimalCapacity).toString(16);
  }

  txSkeleton = txSkeleton.update("outputs", (outputs) => {
    return outputs.push(targetOutput);
  });
  txSkeleton = txSkeleton.update("fixedEntries", (fixedEntries) => {
    return fixedEntries.push({
      field: "outputs",
      index: txSkeleton.get("outputs").size - 1,
    });
  });

  // build input cells
  const fromScripts: Script[] = fromInfos.map(
    (fromInfo) => parseFromInfo(fromInfo, { config }).fromScript
  );

  const changeOutputLockScript = changeAddress
    ? parseAddress(changeAddress, { config })
    : fromScripts[0];
  let previousInputs = Set<string>();
  let discardChangeCellFlag = false;
  if (sudt) {
    // collect enough sudt input cells
    const changeCell: Cell = {
      cell_output: {
        capacity: "0x0",
        lock: changeOutputLockScript,
        type: sudtTypeScript,
      },
      data: toBigUInt128LE(0n),
      out_point: undefined,
      block_hash: undefined,
    };
    const targetOutputSudtAmount = sudt.amount;
    const targetOutputCkbCapacity = BigInt(targetOutput.cell_output.capacity);
    // TODO: add support for tipHeader
    const result = await collectSudtAndCkb(
      txSkeleton,
      fromInfos,
      sudtTypeScript,
      cellProvider,
      targetOutputCkbCapacity,
      targetOutputSudtAmount,
      previousInputs,
      { config }
    );
    txSkeleton = result.txSkeleton;
    previousInputs = result.previousInputs;
    const inputSudtAmountSum = result.inputSudtAmountSum;
    const inputCkbCapacitySum = result.inputCkbCapacitySum;

    if (inputSudtAmountSum < targetOutputSudtAmount) {
      throw new Error("Insufficient sudt amount in fromInfos");
    } else if (inputSudtAmountSum === targetOutputSudtAmount) {
      // update changeCell's type script and data, no sudt change
      changeCell.data = "0x";
      changeCell.cell_output.type = undefined;
      if (inputCkbCapacitySum === targetOutputCkbCapacity) {
        // no ckb change, discard the changeCell
        discardChangeCellFlag = true;
      } else {
        const changeCellMinimalCapacity = minimalCellCapacity(changeCell);
        if (
          inputCkbCapacitySum >=
          targetOutputCkbCapacity + changeCellMinimalCapacity
        ) {
          // input cells capacity is sufficient for both targetOutputCell and changeCell
          changeCell.cell_output.capacity =
            "0x" + (inputCkbCapacitySum - targetOutputCkbCapacity).toString(16);
        } else {
          const extraRequiredCkbCapacity =
            targetOutputCkbCapacity +
            changeCellMinimalCapacity -
            inputCkbCapacitySum;
          const result = await collectSudtAndCkb(
            txSkeleton,
            fromInfos,
            "empty",
            cellProvider,
            extraRequiredCkbCapacity,
            0n,
            previousInputs,
            { config }
          );
          txSkeleton = result.txSkeleton;
          previousInputs = result.previousInputs;
          const inputCkbCapacitySum2 = result.inputCkbCapacitySum;
          if (inputCkbCapacitySum2 < extraRequiredCkbCapacity) {
            throw new Error("Insufficient ckb amount in fromInfos");
          } else {
            changeCell.cell_output.capacity =
              "0x" +
              (
                inputCkbCapacitySum +
                inputCkbCapacitySum2 -
                targetOutputCkbCapacity
              ).toString(16);
          }
        }
      }
    } else {
      // update changeCell's sudt value
      changeCell.data = toBigUInt128LE(
        inputSudtAmountSum - targetOutputSudtAmount
      );
      const changeCellMinimalCapacity = minimalCellCapacity(changeCell);
      if (
        inputCkbCapacitySum >=
        targetOutputCkbCapacity + changeCellMinimalCapacity
      ) {
        // input cells capacity is sufficient for both targetOutputCell and changeCell
        changeCell.cell_output.capacity =
          "0x" + (inputCkbCapacitySum - targetOutputCkbCapacity).toString(16);
      } else {
        const extraRequiredCkbCapacity =
          targetOutputCkbCapacity +
          changeCellMinimalCapacity -
          inputCkbCapacitySum;
        const result = await collectSudtAndCkb(
          txSkeleton,
          fromInfos,
          "empty",
          cellProvider,
          extraRequiredCkbCapacity,
          0n,
          previousInputs,
          { config }
        );
        txSkeleton = result.txSkeleton;
        previousInputs = result.previousInputs;
        const inputCkbCapacitySum2 = result.inputCkbCapacitySum;
        if (inputCkbCapacitySum2 < extraRequiredCkbCapacity) {
          throw new Error("Insufficient ckb amount in fromInfos");
        } else {
          changeCell.cell_output.capacity =
            "0x" +
            (
              inputCkbCapacitySum +
              inputCkbCapacitySum2 -
              targetOutputCkbCapacity
            ).toString(16);
        }
      }
    }
    if (!discardChangeCellFlag) {
      txSkeleton = txSkeleton.update("outputs", (outputs) =>
        outputs.push(changeCell)
      );
    }
  } else {
    // without sudt, only need deal with capacity
    // collect enough ckb input cells
    const changeCell: Cell = {
      cell_output: {
        capacity: "0x0",
        lock: changeOutputLockScript,
        type: undefined,
      },
      data: "0x",
      out_point: undefined,
      block_hash: undefined,
    };
    const targetOutputSudtAmount = 0n;
    const targetOutputCkbCapacity = BigInt(targetOutput.cell_output.capacity);

    const result = await collectSudtAndCkb(
      txSkeleton,
      fromInfos,
      "empty",
      cellProvider,
      targetOutputCkbCapacity,
      targetOutputSudtAmount,
      previousInputs,
      { config }
    );
    txSkeleton = result.txSkeleton;
    previousInputs = result.previousInputs;
    const inputCkbCapacitySum = result.inputCkbCapacitySum;
    if (inputCkbCapacitySum < targetOutputCkbCapacity) {
      throw new Error("Insufficient ckb amount in fromInfos");
    } else if (inputCkbCapacitySum === targetOutputCkbCapacity) {
      // no ckb change, discard the changeCell
      discardChangeCellFlag = true;
    } else {
      changeCell.cell_output.capacity =
        "0x" + (inputCkbCapacitySum - targetOutputCkbCapacity).toString(16);
    }
    if (!discardChangeCellFlag) {
      txSkeleton = txSkeleton.update("outputs", (outputs) =>
        outputs.push(changeCell)
      );
    }

    //txSkeleton = await common.injectCapacity(
    //  txSkeleton,
    //  fromInfos,
    //  BigInt(targetOutput.cell_output.capacity),
    //  changeAddress,
    //  undefined,
    //  //tipHeader,
    //  {
    //    config,
    //  }
    //);

    //const tx = sealTransaction(txSkeleton, []);
    //for (const input of txSkeleton.get("inputs")) {
    //  console.log(BigInt(input.cell_output.capacity));
    //}
    //for (const output of txSkeleton.get("outputs")) {
    //  console.log(BigInt(output.cell_output.capacity));
    //}
    //console.log(tx);
  }
  return txSkeleton;
}

/**
 *  Aggregator submit L2BLock with/without depostions and withdraws
 */
export async function submitL2Block(
  txSkeleton: TransactionSkeletonType,
  l2Block: Hash,
  depositions: DepositionRequest[],
  withdraws: WithdrawalRequest[],
  postGlobalState: GlobalState,
  blockHash: Hash,
  blockNumber: bigint,
  { config = undefined }: Options = {}
): Promise<TransactionSkeletonType> {
  config = config || getConfig();
  const ROLLUP_CUSTODIAN_LOCK = config.SCRIPTS.ROLLUP_CUSTODIAN_LOCK!;
  const ROLLUP_TYPE = config.SCRIPTS.ROLLUP_TYPE!;
  // always success lock
  const ROLLUP_ALLWAYS_SUCCESS_LOCK = config.SCRIPTS
    .ROLLUP_ALLWAYS_SUCCESS_LOCK!;

  // update txSkeleton's cell_dep: add rollup type script dep
  txSkeleton = addCellDep(txSkeleton, {
    out_point: {
      tx_hash: ROLLUP_TYPE.TX_HASH,
      index: ROLLUP_TYPE.INDEX,
    },
    dep_type: ROLLUP_TYPE.DEP_TYPE,
  });
  // update txSkeleton's cell_dep: add rollup lock script dep
  txSkeleton = addCellDep(txSkeleton, {
    out_point: {
      tx_hash: ROLLUP_ALLWAYS_SUCCESS_LOCK.TX_HASH,
      index: ROLLUP_ALLWAYS_SUCCESS_LOCK.INDEX,
    },
    dep_type: ROLLUP_ALLWAYS_SUCCESS_LOCK.DEP_TYPE,
  });
  // build updated rollup cell
  const rollupLockScript: Script = {
    code_hash: ROLLUP_ALLWAYS_SUCCESS_LOCK.CODE_HASH,
    hash_type: ROLLUP_ALLWAYS_SUCCESS_LOCK.HASH_TYPE,
    args: "0x",
  };
  const rollupTypeScript: Script = {
    code_hash: ROLLUP_TYPE.CODE_HASH,
    hash_type: ROLLUP_TYPE.HASH_TYPE,
    args: "0x",
  };
  const rollupTypeHash: Hash = computeScriptHash(rollupTypeScript);
  const data = encodeGlobalState(postGlobalState);
  let rollupCellOutput: Cell = {
    cell_output: {
      capacity: "0x0",
      lock: rollupLockScript,
      type: rollupTypeScript,
    },
    data: data,
    out_point: undefined,
    block_hash: undefined,
  };
  const rollupCellCapacity = minimalCellCapacity(rollupCellOutput);
  rollupCellOutput.cell_output.capacity =
    "0x" + BigInt(rollupCellCapacity).toString(16);
  txSkeleton = txSkeleton.update("outputs", (outputs) => {
    return outputs.push(rollupCellOutput);
  });

  if (depositions.length > 0) {
    // update txSkeleton's cell_dep: add deposition_lock_script dep
    txSkeleton = addCellDep(txSkeleton, {
      out_point: {
        tx_hash: ROLLUP_CUSTODIAN_LOCK.TX_HASH,
        index: ROLLUP_CUSTODIAN_LOCK.INDEX,
      },
      dep_type: ROLLUP_CUSTODIAN_LOCK.DEP_TYPE,
    });
    for (const deposition of depositions) {
      const args = encodeCustodianLockArgs(
        rollupTypeHash,
        deposition.ownerLockHash,
        blockHash,
        blockNumber
      );
      const custodianLockScript: Script = {
        code_hash: ROLLUP_CUSTODIAN_LOCK.CODE_HASH,
        hash_type: ROLLUP_CUSTODIAN_LOCK.HASH_TYPE,
        args: args,
      };
      let custodianCellOutput: Cell = {
        cell_output: {
          capacity: "0x0",
          lock: custodianLockScript,
          type: undefined,
        },
        data: "0x",
        out_point: undefined,
        block_hash: undefined,
      };
      const minimalCapacity = minimalCellCapacity(custodianCellOutput);
      if (deposition.sudt_script.code_hash === L2_CKB_SCRIPT.code_hash) {
        if (deposition.amount < minimalCapacity) {
          throw new Error(
            "deposotion ckb amount must be larger than minimalCapacity as it's synced from ckb mainnet"
          );
        }
        custodianCellOutput.cell_output.capacity =
          "0x" + deposition.amount.toString(16);
      } else {
      }
    }
  }

  if (withdraws.length > 0) {
    // TODO
  }

  return txSkeleton;
}

/**
 * User cancel uncollect deposition after cancel_timeout
 */
export async function cancleDeposit(
  txSkeleton: TransactionSkeletonType
): Promise<TransactionSkeletonType> {
  return txSkeleton;
}

// Input cell collection strategy:
// terminate loop when enough sudt and ckb are collected,
// if the total input cells' sudt is insufficient will throw error,
// TODO if the total input cells' sudt is enough but ckb is insufficient, recursively call collectSudtAndCkb with targetSudtAmount equals 0n.
async function collectSudtAndCkb(
  txSkeleton: TransactionSkeletonType,
  fromInfos: FromInfo[],
  sudtTypeScript: Script | "empty" | undefined,
  cellProvider: CellProvider,
  targetCkbCapacity: bigint,
  targetSudtAmount: bigint,
  previousInputs: Set<string>,
  { config = undefined }: Options = {}
): Promise<{
  txSkeleton: TransactionSkeletonType;
  inputSudtAmountSum: bigint;
  inputCkbCapacitySum: bigint;
  previousInputs: Set<string>;
}> {
  let inputSudtAmountSum = 0n;
  let inputCkbCapacitySum = 0n;
  const getInputKey = (input: Cell) =>
    `${input.out_point!.tx_hash}_${input.out_point!.index}`;
  config = config || getConfig();
  for (let index = 0; index < fromInfos.length; index++) {
    const fromScript = parseFromInfo(fromInfos[index], { config }).fromScript;
    const queryOptions = {
      lock: fromScript,
      type: sudtTypeScript,
      data: "any",
    };
    const cellCollector = cellProvider.collector(queryOptions);
    for await (const cell of cellCollector.collect()) {
      const key = getInputKey(cell);
      if (previousInputs.has(key)) {
        continue;
      }
      previousInputs = previousInputs.add(key);
      let inputSudtAmount = 0n;
      if (sudtTypeScript && sudtTypeScript != "empty") {
        inputSudtAmount = readBigUInt128LE(cell.data);
      }
      const inputCkbCapacity = BigInt(cell.cell_output.capacity);
      inputSudtAmountSum += inputSudtAmount;
      inputCkbCapacitySum += inputCkbCapacity;
      // add input cell TODO update this part
      txSkeleton = await common.setupInputCell(
        txSkeleton,
        cell,
        fromInfos[index],
        {
          config,
        }
      );
      // remove unnecessary txSkeleton data introduced by above step
      const lastOutputIndex: number = txSkeleton.get("outputs").size - 1;
      txSkeleton = txSkeleton.update("outputs", (outputs) => {
        return outputs.remove(lastOutputIndex);
      });
      const fixedEntryIndex: number = txSkeleton
        .get("fixedEntries")
        .findIndex((fixedEntry) => {
          return (
            fixedEntry.field === "outputs" &&
            fixedEntry.index === lastOutputIndex
          );
        });
      if (fixedEntryIndex >= 0) {
        txSkeleton = txSkeleton.update("fixedEntries", (fixedEntries) => {
          return fixedEntries.remove(fixedEntryIndex);
        });
      }
      if (
        inputSudtAmountSum >= targetSudtAmount &&
        inputCkbCapacitySum >= targetCkbCapacity
      ) {
        break;
      }
    }
    if (
      inputSudtAmountSum >= targetSudtAmount &&
      inputCkbCapacitySum >= targetCkbCapacity
    ) {
      break;
    }
  }
  return {
    txSkeleton,
    inputSudtAmountSum,
    inputCkbCapacitySum,
    previousInputs,
  };
}

function encodeDepositionLockArgs(
  rollupTypeHash: Hash,
  l2LockScript: Script,
  ownerLockHash: Hash,
  cancelTimeout: bigint
): Hash {
  //TODO
  return "0x";
}

function encodeRollupCustodianLockArgs(
  rollupTypeHash: Hash,
  ownerLockHash: Hash,
  blockHash: Hash,
  blockNumber: bigint
): Hash {
  return "0x";
}

function encodeGlobalState(golbalState: GlobalState): Hash {
  return "0x";
}
function encodeCustodianLockArgs(
  rollupTypeHash: Hash,
  ownerLockHash: Hash,
  blockHash: Hash,
  blockNumber: bigint
): Hash {
  return "0x";
}
export default {
  deposit,
};
