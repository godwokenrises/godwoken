import { Hash, HexString } from "@ckb-lumos/base";
import Knex, { Knex as KnexType } from "knex";
import { rlp } from "ethereumjs-util";
import { DBTransaction } from "../src/db/types";
import commander from "commander";
import dotenv from "dotenv";
import keccak256 from "keccak256";
import { RPC } from "@ckb-lumos/toolkit";

export async function fixEthTxHashRun(program: commander.Command) {
  try {
    let databaseUrl = (program as any).databaseUrl;
    if (databaseUrl == null) {
      dotenv.config({ path: "./.env" });
      databaseUrl = process.env.DATABASE_URL;
    }

    if (databaseUrl == null) {
      throw new Error("Please provide --database-url");
    }

    let chainId = (program as any).chainId;
    if (chainId == null) {
      const rpc = new RPC((program as any).rpc);
      let nodeInfo;
      try {
        nodeInfo = await rpc.gw_get_node_info();
      } catch (e) {
        console.error(e);
        throw new Error(
          "Get chain id from RPC failed, Please provide --chain-id"
        );
      }
      chainId = nodeInfo?.rollup_config?.chain_id;
    }

    if (chainId == null) {
      throw new Error("Please provide --chain-id");
    }

    await fixEthTxHash(databaseUrl, BigInt(chainId));
    process.exit(0);
  } catch (e) {
    console.error(e);
    process.exit(1);
  }
}

export async function listWrongEthTxHashesRun(
  program: commander.Command
): Promise<void> {
  try {
    let databaseUrl = (program as any).databaseUrl;
    if (databaseUrl == null) {
      dotenv.config({ path: "./.env" });
      databaseUrl = process.env.DATABASE_URL;
    }
    if (databaseUrl == null) {
      throw new Error("Please provide --database-url");
    }
    await listWrongEthTxHashes(databaseUrl);
    process.exit(0);
  } catch (e) {
    console.error(e);
    process.exit(1);
  }
}

// fix for leading zeros
export async function fixEthTxHash(
  databaseUrl: string,
  chainId: bigint
): Promise<void> {
  const knex = getKnex(databaseUrl);

  const query = knex<DBTransaction>("transactions").whereRaw(
    `r like ('\\x00')::bytea||'%' or s like ('\\x00')::bytea||'%'`
  );
  const sql = query.toSQL();
  console.log("Query SQL:", sql.sql);
  const wrongTxs = await query;

  console.log(`Found ${wrongTxs.length} wrong txs`);
  await knex.transaction(async (trx) => {
    await Promise.all(
      wrongTxs.map(async (tx: DBTransaction) => {
        const gwTxHash: Hash = bufferToHex(tx.hash);
        const originEthTxHash: Hash = bufferToHex(tx.eth_tx_hash);
        const newEthTxHash: Hash = updateEthTxHash(tx, chainId);
        await trx<DBTransaction>("transactions")
          .update({ eth_tx_hash: Buffer.from(newEthTxHash.slice(2), "hex") })
          .where({ eth_tx_hash: tx.eth_tx_hash });
        console.log(
          `update gw_tx_hash: (${gwTxHash})'s eth_tx_hash, (${originEthTxHash}) --> (${newEthTxHash})`
        );
      })
    );
  });
  console.log(`All ${wrongTxs.length} txs updated!`);
}

async function listWrongEthTxHashes(databaseUrl: string): Promise<void> {
  const knex = getKnex(databaseUrl);

  const query = knex<DBTransaction>("transactions").whereRaw(
    `r like ('\\x00')::bytea||'%' or s like ('\\x00')::bytea||'%'`
  );
  const sql = query.toSQL();
  console.log("Query SQL:", sql.sql);
  const wrongTxs = await query.limit(20);
  const txCount = await query.count();

  for (const tx of wrongTxs) {
    console.log({
      gw_tx_hash: bufferToHex(tx.hash),
      eth_tx_hash: bufferToHex(tx.eth_tx_hash),
      r: bufferToHex(tx.r),
      s: bufferToHex(tx.s),
    });
  }

  console.log(`Found ${txCount[0].count} wrong txs`);
}

function getKnex(databaseUrl: string): KnexType {
  const knex = Knex({
    client: "postgresql",
    connection: {
      connectionString: databaseUrl,
      keepAlive: true,
    },
    pool: { min: 2, max: 20 },
  });
  return knex;
}

const bufferToHex = (buf: Buffer): HexString => "0x" + buf.toString("hex");

function updateEthTxHash(tx: DBTransaction, chainId: bigint): Hash {
  const nonce: bigint = BigInt(tx.nonce || "0");
  const gasPrice: bigint = BigInt(tx.gas_price || "0");
  const gasLimit: bigint = BigInt(tx.gas_limit || "0");
  const value = BigInt(tx.value);
  const r = BigInt("0x" + tx.r.toString("hex"));
  const s = BigInt("0x" + tx.s.toString("hex"));

  // for non eip-155 txs(chain id = 0), v = 27 + last byte of signature(0 / 1)
  // for eip-155 txs(chain id != 0), v = chain_id * 2 + 35 + last byte of signature(0 / 1)
  const v: bigint =
    chainId === 0n ? 27n + BigInt(tx.v) : chainId * 2n + 35n + BigInt(tx.v);

  const data = "0x" + tx.input?.toString("hex");
  const to =
    tx.to_address == null ? "0x" : "0x" + tx.to_address.toString("hex");

  const rlpEncodeData: rlp.Input = [
    nonce,
    gasPrice,
    gasLimit,
    to,
    value,
    data,
    v,
    r,
    s,
  ];
  const rlpEncoded = "0x" + rlp.encode(rlpEncodeData).toString("hex");
  const ethTxHash: Hash = calcEthTxHash(rlpEncoded);
  return ethTxHash;
}

export function calcEthTxHash(encodedSignedTx: HexString): Hash {
  const ethTxHash =
    "0x" +
    keccak256(Buffer.from(encodedSignedTx.slice(2), "hex")).toString("hex");
  return ethTxHash;
}
