import test from "ava";
import { CellProvider } from "./cell_provider";

test("godwoken deposit only CKB to Godwoken network", async (t) => {
    const cellProvider = new CellProvider(bobSecpInputs);
    let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
      cellProvider,
    });

    txSkeleton = await godwoken.deposit(
        txSkeleton,
        [bob.testnetAddress],
        aliace.testnetAddress,
        bob.secpLockHash,
        cancelTimeout,
        10n,
        undefined,
        bob.testnetAddress,
        undefined,
        { config: AGGRON4 }
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

    t.pass();
});

test("godwoken deposit only SUDT to Godwoken network", async (t) => {
    t.pass();
});

test("godwoken deposit both CKB and SUDT to Godwoken network", async (t) => {
    t.pass();
});

test("godwoken failed to deposit SUDT to Godwoken network for insufficient balance", async (t) => {
    t.pass();
});

test("godwoken failed to deposit CKB to Godwoken network for insufficient balance", async (t) => {
    t.pass();
});
