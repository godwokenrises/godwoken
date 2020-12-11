import test from "ava";
import { Cell, Script, utils, Hash } from "@ckb-lumos/base";
import {
  TransactionSkeletonType,
  TransactionSkeleton,
} from "@ckb-lumos/helpers";
import { deposit } from "../src/transaction_builder";
import { alice, bob } from "./account_info";
import { CellProvider } from "./cell_provider";
import { CONFIG } from "./config";
import { bobUnspentCells } from "./cells";

test("godwoken deposit only CKB to Godwoken network", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  const capacity = BigInt(80 * 10 ** 8);
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    capacity,
    undefined,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);
});

test("godwoken deposit only SUDT to Godwoken network", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  const sudt = {
    tokenId:
      "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    amount: BigInt(800 * 10 ** 8),
  };
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    undefined,
    sudt,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);

  const sumOfInputSudt = txSkeleton
    .get("inputs")
    .map((i) => {
      if (i.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(i.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputSudt = txSkeleton
    .get("outputs")
    .map((o) => {
      if (o.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(o.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputSudt, sumOfInputSudt);
});

test("godwoken deposit only CKB to Godwoken network, without changeOutputCell", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  const capacity = BigInt(100 * 10 ** 8);
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    capacity,
    undefined,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);
  t.is(sumOfInputCapacity, capacity);
  t.is(txSkeleton.get("outputs").size, 1);
});

test("godwoken deposit both CKB and SUDT to Godwoken network", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  const capacity = BigInt(80 * 10 ** 8);
  const sudt = {
    tokenId:
      "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    amount: BigInt(800 * 10 ** 8),
  };
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    capacity,
    sudt,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);

  const sumOfInputSudt = txSkeleton
    .get("inputs")
    .map((i) => {
      if (i.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(i.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputSudt = txSkeleton
    .get("outputs")
    .map((o) => {
      if (o.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(o.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputSudt, sumOfInputSudt);
});

test("godwoken deposit both CKB and SUDT to Godwoken network, with changeOutput including ckb and no sudt", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  const capacity = BigInt(80 * 10 ** 8);
  const sudt = {
    tokenId:
      "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    amount: BigInt(1500 * 10 ** 8),
  };
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    capacity,
    sudt,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);

  const sumOfInputSudt = txSkeleton
    .get("inputs")
    .map((i) => {
      if (i.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(i.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputSudt = txSkeleton
    .get("outputs")
    .map((o) => {
      if (o.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(o.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputSudt, sumOfInputSudt);
  const changeOutputCell = txSkeleton.get("outputs").get(1)!;
  t.is(changeOutputCell.cell_output.type, undefined);
  t.is(changeOutputCell.data, "0x");
});

test("godwoken deposit both CKB and SUDT to Godwoken network, without changeOutputCell", async (t) => {
  const cellProvider = new CellProvider(bobUnspentCells);
  let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
    cellProvider,
  });

  const cancelTimeout = BigInt(10);
  // 130 is larger than the minimalCapacity of sudt cell
  const capacity = BigInt(130 * 10 ** 8);
  const sudt = {
    tokenId:
      "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    amount: BigInt(1500 * 10 ** 8),
  };
  txSkeleton = await deposit(
    txSkeleton,
    [bob.testnetAddress],
    alice.testnetAddress,
    bob.secpLockHash,
    cancelTimeout,
    capacity,
    sudt,
    bob.testnetAddress,
    //undefined,
    { config: CONFIG }
  );

  const sumOfInputCapacity = txSkeleton
    .get("inputs")
    .map((i) => BigInt(i.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputCapacity = txSkeleton
    .get("outputs")
    .map((o) => BigInt(o.cell_output.capacity))
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputCapacity, sumOfInputCapacity);

  const sumOfInputSudt = txSkeleton
    .get("inputs")
    .map((i) => {
      if (i.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(i.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  const sumOfOutputSudt = txSkeleton
    .get("outputs")
    .map((o) => {
      if (o.cell_output.type) {
        return BigInt(utils.readBigUInt128LE(o.data));
      } else {
        return BigInt(0);
      }
    })
    .reduce((result, c) => result + c, BigInt(0));
  t.is(sumOfOutputSudt, sumOfInputSudt);
  t.is(sumOfInputSudt, sudt.amount);
  t.is(sumOfInputCapacity, capacity);
  t.is(txSkeleton.get("outputs").size, 1);
});

test("godwoken failed to deposit CKB to Godwoken network for insufficient balance", async (t) => {
  const error = await t.throwsAsync(
    async () => {
      const cellProvider = new CellProvider(bobUnspentCells);
      let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
        cellProvider,
      });
      const cancelTimeout = BigInt(10);
      const capacity = BigInt(380 * 10 ** 8);
      txSkeleton = await deposit(
        txSkeleton,
        [bob.testnetAddress],
        alice.testnetAddress,
        bob.secpLockHash,
        cancelTimeout,
        capacity,
        undefined,
        bob.testnetAddress,
        //undefined,
        { config: CONFIG }
      );
    },
    { instanceOf: Error }
  );
  t.is(error.message, "Insufficient ckb amount in fromInfos");
});

test("godwoken failed to deposit SUDT to Godwoken network for insufficient balance", async (t) => {
  const error = await t.throwsAsync(
    async () => {
      const cellProvider = new CellProvider(bobUnspentCells);
      let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
        cellProvider,
      });
      const cancelTimeout = BigInt(10);
      const sudt = {
        tokenId:
          "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
        amount: BigInt(2000 * 10 ** 8),
      };
      txSkeleton = await deposit(
        txSkeleton,
        [bob.testnetAddress],
        alice.testnetAddress,
        bob.secpLockHash,
        cancelTimeout,
        undefined,
        sudt,
        bob.testnetAddress,
        //undefined,
        { config: CONFIG }
      );
    },
    { instanceOf: Error }
  );
  t.is(error.message, "Insufficient sudt amount in fromInfos");
});
