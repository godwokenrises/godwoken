import {
  serializeEthAddrRegArgs,
  serializeMetaContractArgs,
  serializeSudtArgs,
} from "../../src/parse-tx";
import test from "ava";
import {
  EthAddrRegArgsType,
  normalizers,
  schemas,
  SudtArgsType,
  MetaContractArgsType,
  CreateAccount,
} from "@godwoken-web3/godwoken";
import { Reader } from "@ckb-lumos/toolkit";
import web3Utils from "web3-utils";
import { MAX_ADDRESS_SIZE_PER_REGISTER_BATCH } from "../../src/methods/constant";
import { calcFee } from "../../src/util";
import { Price } from "../../src/base/gas-price";

const serializedRegSetMappingL2Tx = prepareRegSetMappingTx();
const serializedRegBatchSetMappingL2Tx = prepareRegBatchSetMappingTx();
const serializedSudtTransferL2Tx = prepareSudtTransferTx();
const serializeMetaContractCreateAccountL2Tx =
  prepareMetaContractCreateAccountTx();
const serializeMetaContractBatchCreateEthAccountL2Tx =
  prepareMetaContractBatchCreateEthAccountTx();

const ckbPrice = "0.0038";
const price = new Price(ckbPrice);

test(`${ckbPrice} ckb price fee rate for setMapping`, (t) => {
  const feeRate = price.toFeeRate();
  const requiredFee = calcFee(serializedRegSetMappingL2Tx, feeRate);
  const consumed = web3Utils.fromWei(requiredFee.toString(10), "ether");
  console.log(`${feeRate} fee rate => setMapping tx consume ${consumed} CKB`);
  t.true(+consumed > 0);
});

test(`${ckbPrice} ckb price fee rate for ${MAX_ADDRESS_SIZE_PER_REGISTER_BATCH} hashes batchSetMapping`, (t) => {
  const feeRate = price.toFeeRate();
  const requiredFee = calcFee(serializedRegBatchSetMappingL2Tx, feeRate);
  const consumed = web3Utils.fromWei(requiredFee.toString(10), "ether");
  console.log(
    `${feeRate} fee rate => batchSetMapping tx consume ${consumed} CKB`
  );
  t.true(+consumed > 0);
});

test(`${ckbPrice} ckb price fee rate for sudt transfer`, (t) => {
  const feeRate = price.toFeeRate();
  const requiredFee = calcFee(serializedSudtTransferL2Tx, feeRate);
  const consumed = web3Utils.fromWei(requiredFee.toString(10), "ether");
  console.log(
    `${feeRate} fee rate => sudt transfer tx consume ${consumed} CKB`
  );
  t.true(+consumed > 0);
});

test(`${ckbPrice} ckb price fee rate for MetaContract create account`, (t) => {
  const feeRate = price.toFeeRate();
  const requiredFee = calcFee(serializeMetaContractCreateAccountL2Tx, feeRate);
  const consumed = web3Utils.fromWei(requiredFee.toString(10), "ether");
  console.log(
    `${feeRate} fee rate => MetaContract create account tx consume ${consumed} CKB`
  );
  t.true(+consumed > 0);
});

test(`${ckbPrice} ckb price fee rate for MetaContract batch create eth account`, (t) => {
  const feeRate = price.toFeeRate();
  const requiredFee = calcFee(
    serializeMetaContractBatchCreateEthAccountL2Tx,
    feeRate
  );
  const consumed = web3Utils.fromWei(requiredFee.toString(10), "ether");
  console.log(
    `${feeRate} fee rate => MetaContract batch create eth account tx consume ${consumed} CKB`
  );
  t.true(+consumed > 0);
});

// helper functions
function prepareSudtTransferTx() {
  const sudtTransfer = {
    to_address:
      "0x3991637c340d585858f45c440116aaf2d13580517fc0fffeb67b5bffe35d77d0",
    amount: "0xffffff",
    fee: {
      registry_id: "0x1",
      amount: "0x10",
    },
  };
  const sudtArgs = {
    type: SudtArgsType.SUDTTransfer,
    value: sudtTransfer,
  };
  const serializedSudtArgs = serializeSudtArgs(sudtArgs);
  const sudtL2Tx = {
    raw: {
      chain_id: "0x116e8",
      from_id: "0x10",
      to_id: "0x1",
      nonce: "0xa4",
      args: serializedSudtArgs,
    },
    signature:
      "0xbde03b87b7da48cc186a51f199355346a8173249886da75898159b1d00bb17940a908af2cc753b9003863a35a0bd35287e7c9f103339e05532d2be179d88d41800",
  };

  const serializedSudtL2Tx = new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(sudtL2Tx))
  ).serializeJson();
  return serializedSudtL2Tx;
}

function prepareRegSetMappingTx() {
  const setMapping = {
    gw_script_hash:
      "0x3991637c340d585858f45c440116aaf2d13580517fc0fffeb67b5bffe35d77d0",
    fee: {
      registry_id: "0x1",
      amount: "0x10",
    },
  };
  const ethAddrRegArgs = {
    type: EthAddrRegArgsType.SetMapping,
    value: setMapping,
  };
  const serializedArgs = serializeEthAddrRegArgs(ethAddrRegArgs);
  const regL2Tx = {
    raw: {
      chain_id: "0x116e8",
      from_id: "0x10",
      to_id: "0x2",
      nonce: "0xa4",
      args: serializedArgs,
    },
    signature:
      "0xbde03b87b7da48cc186a51f199355346a8173249886da75898159b1d00bb17940a908af2cc753b9003863a35a0bd35287e7c9f103339e05532d2be179d88d41800",
  };

  const serializedRegL2Tx = new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(regL2Tx))
  ).serializeJson();
  return serializedRegL2Tx;
}

function prepareRegBatchSetMappingTx() {
  const batchSetMapping = {
    gw_script_hashes: new Array(MAX_ADDRESS_SIZE_PER_REGISTER_BATCH).fill(
      "0x3991637c340d585858f45c440116aaf2d13580517fc0fffeb67b5bffe35d77d0"
    ),
    fee: {
      registry_id: "0x1",
      amount: "0x10",
    },
  };
  const ethAddrRegArgs = {
    type: EthAddrRegArgsType.BatchSetMapping,
    value: batchSetMapping,
  };
  const serializedSetMapping = serializeEthAddrRegArgs(ethAddrRegArgs);
  const regL2Tx = {
    raw: {
      chain_id: "0x116e8",
      from_id: "0x10",
      to_id: "0x2",
      nonce: "0xa4",
      args: serializedSetMapping,
    },
    signature:
      "0xbde03b87b7da48cc186a51f199355346a8173249886da75898159b1d00bb17940a908af2cc753b9003863a35a0bd35287e7c9f103339e05532d2be179d88d41800",
  };

  const serializedRegL2Tx = new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(regL2Tx))
  ).serializeJson();
  return serializedRegL2Tx;
}

function prepareMetaContractCreateAccountTx() {
  const createEthAccount: CreateAccount = {
    script: {
      code_hash:
        "0x3991637c340d585858f45c440116aaf2d13580517fc0fffeb67b5bffe35d77d0",
      hash_type: "type",
      args: "0x1111",
    },
    fee: {
      registry_id: "0x1",
      amount: "0x10",
    },
  };
  const metaContractArgs = {
    type: MetaContractArgsType.CreateAccount,
    value: createEthAccount,
  };
  const serializedArgs = serializeMetaContractArgs(metaContractArgs);
  const l2Tx = {
    raw: {
      chain_id: "0x116e8",
      from_id: "0x10",
      to_id: "0x2",
      nonce: "0xa4",
      args: serializedArgs,
    },
    signature:
      "0xbde03b87b7da48cc186a51f199355346a8173249886da75898159b1d00bb17940a908af2cc753b9003863a35a0bd35287e7c9f103339e05532d2be179d88d41800",
  };

  const serializedL2Tx = new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(l2Tx))
  ).serializeJson();
  return serializedL2Tx;
}

function prepareMetaContractBatchCreateEthAccountTx() {
  const batchCreateEthAccount = {
    scripts: new Array(MAX_ADDRESS_SIZE_PER_REGISTER_BATCH).fill({
      code_hash:
        "0x3991637c340d585858f45c440116aaf2d13580517fc0fffeb67b5bffe35d77d0",
      hash_type: "type",
      args: "0x1111",
    }),
    fee: {
      registry_id: "0x1",
      amount: "0x10",
    },
  };
  const metaContractArgs = {
    type: MetaContractArgsType.BatchCreateEthAccounts,
    value: batchCreateEthAccount,
  };
  const serializedSetMapping = serializeMetaContractArgs(metaContractArgs);
  const l2Tx = {
    raw: {
      chain_id: "0x116e8",
      from_id: "0x10",
      to_id: "0x2",
      nonce: "0xa4",
      args: serializedSetMapping,
    },
    signature:
      "0xbde03b87b7da48cc186a51f199355346a8173249886da75898159b1d00bb17940a908af2cc753b9003863a35a0bd35287e7c9f103339e05532d2be179d88d41800",
  };

  const serializedL2Tx = new Reader(
    schemas.SerializeL2Transaction(normalizers.NormalizeL2Transaction(l2Tx))
  ).serializeJson();
  return serializedL2Tx;
}
