export const bob = {
  mainnetAddress: "ckb1qyqrdsefa43s6m882pcj53m4gdnj4k440axqdt9rtd",
  testnetAddress: "ckt1qyqrdsefa43s6m882pcj53m4gdnj4k440axqswmu83",
  blake160: "0x36c329ed630d6ce750712a477543672adab57f4c",
  multisigTestnetAddress: "ckt1qyq4du5pk02tkh788363w990p0mcaw9t5rvq8ywwns",
  multisigArgs: "0x56f281b3d4bb5fc73c751714af0bf78eb8aba0d8",
  acpTestnetAddress:
    "ckt1qjr2r35c0f9vhcdgslx2fjwa9tylevr5qka7mfgmscd33wlhfykykdkr98kkxrtvuag8z2j8w4pkw2k6k4l5cgxhkrr",
  secpLockHash:
    "0x1f2615a8dde4e28ca736ff763c2078aff990043f4cbf09eb4b3a58a140a0862d",
  get fromInfo() {
    return {
      R: 0,
      M: 1,
      publicKeyHashes: [this.blake160],
    };
  },
};

export const alice = {
  mainnetAddress: "ckb1qyqwyxfa75whssgkq9ukkdd30d8c7txct0gq5f9mxs",
  testnetAddress: "ckt1qyqwyxfa75whssgkq9ukkdd30d8c7txct0gqfvmy2v",
  blake160: "0xe2193df51d78411601796b35b17b4f8f2cd85bd0",
  multisigTestnetAddress: "ckt1qyq4zqqvh8alx39hhunc930tmhaqlrtnvkcqnp5xln",
  acpTestnetAddress:
    "ckt1qjr2r35c0f9vhcdgslx2fjwa9tylevr5qka7mfgmscd33wlhfykyhcse8h6367zpzcqhj6e4k9a5lrevmpdaq9kve7y",
  get fromInfo() {
    return {
      R: 0,
      M: 1,
      publicKeyHashes: [this.blake160],
    };
  },
};

export const fullAddressInfo = {
  mainnetAddress:
    "ckb1qsqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpvumhs9nvu786dj9p0q5elx66t24n3kxgmz0sxt",
  testnetAddress:
    "ckt1qsqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpvumhs9nvu786dj9p0q5elx66t24n3kxgkpkap5",
  lock: {
    code_hash:
      "0x0000000000000000000000000000000000000000000000000000000000000000",
    hash_type: "type",
    args: "0xb39bbc0b3673c7d36450bc14cfcdad2d559c6c64",
  },
};
