import { Cell, HexNumber } from "@ckb-lumos/base";
import { TransactionSkeletonType } from "@ckb-lumos/helpers";
import { StateValidatorLockGeneratorState } from "./utils";

export class AlwaysSuccessGenerator {
  lastProduceBlockTime: bigint;

  constructor() {
    this.lastProduceBlockTime = 0n;
  }

  async shouldIssueNewBlock(
    medianTimeHex: HexNumber,
    tipCell: Cell
  ): Promise<StateValidatorLockGeneratorState> {
    // Issue a new block every 35 seconds
    const medianTime = BigInt(medianTimeHex);
    if (medianTime - this.lastProduceBlockTime >= 35n * 1000n) {
      this.lastProduceBlockTime = medianTime;
      return "Yes";
    }
    return "No";
  }

  async fixTransactionSkeleton(
    txSkeleton: TransactionSkeletonType
  ): Promise<TransactionSkeletonType> {
    return txSkeleton;
  }
}
