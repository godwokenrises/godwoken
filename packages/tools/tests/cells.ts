import { Cell, utils, Hash } from "@ckb-lumos/base";
// WARN: cells are mocked for test purpose only.

// 100 CKB
const bobUnspentCell0: Cell = {
  cell_output: {
    capacity: ckbToHex(100),
    lock: {
      code_hash:
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
      hash_type: "type",
      args: "0x36c329ed630d6ce750712a477543672adab57f4c",
    },
    type: undefined,
  },
  data: "0x",
  out_point: {
    tx_hash:
      "0x486ead64a7c2c1a3132c2e03d2af364050f4f0f6dfafad291daa7db6aed53e10",
    index: "0x0",
  },
  block_hash:
    "0x1111111111111111111111111111111111111111111111111111111111111111",
  block_number: "0x1",
};

// 100 CKB
const bobUnspentCell1: Cell = {
  cell_output: {
    capacity: ckbToHex(100),
    lock: {
      code_hash:
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
      hash_type: "type",
      args: "0x36c329ed630d6ce750712a477543672adab57f4c",
    },
    type: undefined,
  },
  data: "0x",
  out_point: {
    tx_hash:
      "0x486ead64a7c2c1a3132c2e03d2af364050f4f0f6dfafad291daa7db6aed53e10",
    index: "0x1",
  },
  block_hash:
    "0x1111111111111111111111111111111111111111111111111111111111111111",
  block_number: "0x1",
};

// 90 CKB
// 1000 SUDT-A
const bobUnspentCell2: Cell = {
  cell_output: {
    capacity: ckbToHex(90),
    lock: {
      code_hash:
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
      hash_type: "type",
      args: "0x36c329ed630d6ce750712a477543672adab57f4c",
    },
    type: {
      code_hash:
        "0x48dbf59b4c7ee1547238021b4869bceedf4eea6b43772e5d66ef8865b6ae7212",
      hash_type: "type",
      args:
        "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    },
  },
  data: sudtToHex(1000),
  out_point: {
    tx_hash:
      "0x6747f0fa9ae72efc75079b5f7b2347f965df0754e22818f511750f1f2d08d2cc",
    index: "0x0",
  },
  block_hash:
    "0x1111111111111111111111111111111111111111111111111111111111111111",
  block_number: "0x1",
};

// 40 CKB
// 500 SUDT-A
const bobUnspentCell3: Cell = {
  cell_output: {
    capacity: ckbToHex(40),
    lock: {
      code_hash:
        "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
      hash_type: "type",
      args: "0x36c329ed630d6ce750712a477543672adab57f4c",
    },
    type: {
      code_hash:
        "0x48dbf59b4c7ee1547238021b4869bceedf4eea6b43772e5d66ef8865b6ae7212",
      hash_type: "type",
      args:
        "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
    },
  },
  data: sudtToHex(500),
  out_point: {
    tx_hash:
      "0x6747f0fa9ae72efc75079b5f7b2347f965df0754e22818f511750f1f2d08d2cc",
    index: "0x1",
  },
  block_hash:
    "0x1111111111111111111111111111111111111111111111111111111111111111",
  block_number: "0x1",
};

export const bobUnspentCells: Cell[] = [
  bobUnspentCell0,
  bobUnspentCell1,
  bobUnspentCell2,
  bobUnspentCell3,
];
export const aliceUnspentCells: Cell[] = [];

function ckbToHex(num: number): Hash {
  return "0x" + BigInt(num * 10 ** 8).toString(16);
}

function sudtToHex(num: number): Hash {
  return utils.toBigUInt128LE(BigInt(num * 10 ** 8));
}
