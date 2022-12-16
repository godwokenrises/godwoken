import test from "ava";
import { MAX_FILTER_TOPIC_ARRAY_LENGTH } from "../src/cache/constant";
import { FilterManager } from "../src/cache";
import { RpcFilterRequest } from "../src/base/filter";
import { globalClient } from "../src/cache/redis";

const EXPIRED_TIMEOUT_MILLISECONDS = 1000;
const manager = new FilterManager(true, EXPIRED_TIMEOUT_MILLISECONDS);

test.beforeEach(async (t) => {
  await globalClient.sendCommand(["FLUSHDB"]);
  t.is(await manager.size(), 0);
});

// FIXME Uncomment this case after fixing the bug
// test.serial("install with address less than 20 bytes-length", async (t) => {
//   const invalid: RpcFilterRequest = {
//     address: "0x0000",
//     fromBlock: "0x123",
//     toBlock: "latest",
//     topics: [
//       "0x0001020000000000000000000000000000000000000000000000000000000000",
//       "0x0000000000000000000000000000000000000000000000000000000000000000",
//     ],
//   };
//   const err = await t.throwsAsync(async () => {
//     await manager.install(invalid, BigInt(0));
//   });
//   t.is(
//     err?.message,
//     `invalid argument 0: address must be a 20 bytes-length hex string`
//   );
// });

test.serial("filter topics exceeds limit", async (t) => {
  const invalid: RpcFilterRequest = {
    address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
    fromBlock: "0x123",
    toBlock: "latest",
    topics: Array(MAX_FILTER_TOPIC_ARRAY_LENGTH + 1).fill(
      "0x0001020000000000000000000000000000000000000000000000000000000000"
    ),
  };
  const err = await t.throwsAsync(async () => {
    await manager.install(invalid, BigInt(0));
  });
  t.is(
    err?.message,
    `got FilterTopics.length ${invalid.topics?.length}, expect limit: ${MAX_FILTER_TOPIC_ARRAY_LENGTH}`
  );

  const valid: RpcFilterRequest = {
    address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
    fromBlock: "0x123",
    toBlock: "latest",
    topics: Array(MAX_FILTER_TOPIC_ARRAY_LENGTH).fill(
      "0x0001020000000000000000000000000000000000000000000000000000000000"
    ),
  };
  await manager.install(valid, BigInt(0));
});

test.serial("filter topic items exceeds limit", async (t) => {
  const invalid: RpcFilterRequest = {
    address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
    fromBlock: "0x123",
    toBlock: "0x520",
    topics: [
      "0x00000000f0000000000000000000000000000000000000000000000000000000",
      Array(MAX_FILTER_TOPIC_ARRAY_LENGTH + 1).fill(
        "0x0001020000000000000000000000000000000000000000000000000000000000"
      ),
    ],
  };
  const err = await t.throwsAsync(async () => {
    await manager.install(invalid, BigInt(0));
  });
  t.is(
    err?.message,
    `got one or more topic item's length ${
      invalid.topics![1]!.length
    }, expect limit: ${MAX_FILTER_TOPIC_ARRAY_LENGTH}`
  );

  const valid: RpcFilterRequest = {
    address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
    fromBlock: "0x123",
    toBlock: "0x520",
    topics: [
      "0x00000000f0000000000000000000000000000000000000000000000000000000",
      Array(MAX_FILTER_TOPIC_ARRAY_LENGTH).fill(
        "0x0001020000000000000000000000000000000000000000000000000000000000"
      ),
    ],
  };
  await manager.install(valid, BigInt(0));
});

test.serial("run the complete filter workflow", async (t) => {
  const RpcFilterRequests: RpcFilterRequest[] = [
    {
      address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
      fromBlock: "0x123",
      toBlock: "latest",
      topics: [
        "0x0001020000000000000000000000000000000000000000000000000000000000",
        "0x0000000000000000000000000000000000000000000000000000000000000000",
      ],
    },
    {
      address: "0x92384EF7176DA84a957A9FE9119585AB2dc7c57d",
      fromBlock: "0x123",
      toBlock: "0x520",
      topics: [
        "0x00000000f0000000000000000000000000000000000000000000000000000000",
        "0x0000000000000000000000000000000000000000000000000000000000000000",
      ],
    },
  ];

  let ids = RpcFilterRequests.map(async (RpcFilterRequest) => {
    return await manager.install(RpcFilterRequest, BigInt(0));
  });
  t.is(await manager.size(), ids.length);
  t.is(await manager.size(), RpcFilterRequests.length);

  for (let i = 0; i < ids.length; i++) {
    const id = await ids[i];

    // filter get
    const actual = await manager.get(id);
    t.deepEqual(actual, RpcFilterRequests[i]);

    // filter getLastPoll
    t.is((await manager.getLastPoll(id)).toString(), "0");

    // filter updateLastPoll
    await manager.updateLastPoll(id, BigInt(25));
    t.is((await manager.getLastPoll(id)).toString(), "25");

    // filter uninstall
    t.true(await manager.uninstall(id));
    t.false(await manager.uninstall(id));
    t.is(await manager.size(), ids.length - i - 1);
  }
});
