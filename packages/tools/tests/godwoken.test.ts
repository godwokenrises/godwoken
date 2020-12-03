import test from "ava";
import { TransactionSkeletonType, TransactionSkeleton } from "@ckb-lumos/helpers";
import { deposit } from "../src/transaction_builder";
import { alice, bob } from "./account_info";
import { CellProvider } from "./cell_provider";
import { CONFIG } from "./config";
import { bobUnspentCells } from "./cells"
test("godwoken deposit only CKB to Godwoken network", async (t) => {
    const cellProvider = new CellProvider(bobUnspentCells);
    let txSkeleton: TransactionSkeletonType = TransactionSkeleton({
      cellProvider,
    });

    const cancelTimeout = BigInt(10);
    const capacity = BigInt(10);
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
