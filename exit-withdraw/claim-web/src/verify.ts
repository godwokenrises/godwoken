import { blockchain, utils } from "@ckb-lumos/base";
import { predefined } from "@ckb-lumos/config-manager";
import { parseAddress } from "@ckb-lumos/helpers";
import { getBytes, hexlify, verifyMessage } from "ethers";
import type {
  BuildRequest,
  Config,
  Output,
  Quote,
  Script,
  Transaction,
} from "./types";
export const OMNI =
  "0x9b819793a64463aed77c615d6cb226eea5487ccfc0783043a587254cda2b6f26";
export const SUDT =
  "0x5e7a36a77e68eecc013dfa2fe6a23f3b6c344b04005808694ae6dd45eea4cfd5";
function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const packedScript = (s: Script) => ({
  codeHash: s.code_hash,
  hashType: s.hash_type,
  args: s.args,
});
const point = (p: { tx_hash: string; index: string }) => ({
  txHash: p.tx_hash,
  index: p.index,
});
const bytes = (h: string) => getBytes(h);
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => [k, canonical(v)]),
    );
  return value;
}
const same = (a: unknown, b: unknown) =>
  JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
export function destination(address: string): Script {
  assert(
    address.toLowerCase().startsWith("ckb1") && address.length <= 2048,
    "Enter a valid CKB mainnet address beginning with ckb1.",
  );
  const s = parseAddress(address, { config: predefined.LINA });
  return { code_hash: s.codeHash, hash_type: s.hashType, args: s.args };
}
function equalScript(a: Script, b: Script) {
  return (
    a.code_hash === b.code_hash &&
    a.hash_type === b.hash_type &&
    a.args === b.args
  );
}
export function minimum(o: Output, data: string) {
  return (
    BigInt(
      8 +
        33 +
        bytes(o.lock.args).length +
        (o.type ? 33 + bytes(o.type.args).length : 0) +
        bytes(data).length,
    ) * 100000000n
  );
}
export function rawTransaction(tx: Transaction) {
  return {
    version: tx.version,
    cellDeps: tx.cell_deps.map((d) => ({
      outPoint: point(d.out_point),
      depType: d.dep_type,
    })),
    headerDeps: tx.header_deps,
    inputs: tx.inputs.map((i) => ({
      since: i.since,
      previousOutput: point(i.previous_output),
    })),
    outputs: tx.outputs.map((o) => ({
      capacity: o.capacity,
      lock: packedScript(o.lock),
      type: o.type ? packedScript(o.type) : undefined,
    })),
    outputsData: tx.outputs_data,
  };
}
function le(n: bigint, size: number) {
  const b = new Uint8Array(size);
  for (let i = 0; i < size; i++) {
    b[i] = Number(n & 255n);
    n >>= 8n;
  }
  assert(n === 0n, "Integer overflow");
  return b;
}
function uint(h: string) {
  return bytes(h).reduceRight((n, x) => n * 256n + BigInt(x), 0n);
}
function token(t: Script) {
  return utils.computeScriptHash(packedScript(t));
}
export function placeholder() {
  const b = new Uint8Array(85);
  [85, 16, 85, 85, 65].forEach((n, i) => b.set(le(BigInt(n), 4), i * 4));
  return hexlify(blockchain.WitnessArgs.pack({ lock: hexlify(b) }));
}
export function digest(tx: Transaction) {
  const raw = rawTransaction(tx);
  const hash = utils.ckbHash(blockchain.RawTransaction.pack(raw));
  const h = new utils.CKBHasher();
  h.update(hash);
  for (let i = 0; i < tx.witnesses.length; i++) {
    const w =
      i === 0
        ? blockchain.WitnessArgs.pack({ lock: hexlify(new Uint8Array(85)) })
        : bytes(tx.witnesses[i]);
    h.update(hexlify(le(BigInt(w.byteLength), 8)));
    h.update(hexlify(w));
  }
  return {
    hash,
    message: h.digestHex(),
    size:
      blockchain.Transaction.pack({ ...raw, witnesses: tx.witnesses })
        .byteLength + 4,
  };
}
/** Validate the exact recipient, owner, assets, fee and personal_sign digest in the browser too. */
export function verifyQuote(
  q: Quote,
  request: BuildRequest,
  config: Pick<
    Config,
    "assets" | "dependencies" | "fee_rate" | "max_fee_shannons"
  >,
) {
  const tx = q.transaction;
  const dest = destination(request.destination);
  const owner: Script = {
    code_hash: OMNI,
    hash_type: "type",
    args: `0x01${request.address.slice(2).toLowerCase()}00`,
  };
  assert(
    q.address === request.address.toLowerCase() &&
      q.destination === request.destination &&
      equalScript(q.destination_lock, dest),
    "Quote identity changed.",
  );
  assert(
    tx.version === "0x0" && tx.header_deps.length === 0,
    "Unexpected transaction fields.",
  );
  assert(
    same(
      tx.cell_deps,
      config.dependencies.map((d) => ({
        out_point: d.out_point,
        dep_type: d.dep_type,
      })),
    ),
    "Dependencies changed.",
  );
  assert(
    request.inputs.length > 0 &&
      request.inputs.length <= 64 &&
      tx.inputs.length === request.inputs.length &&
      q.input_cells.length === tx.inputs.length,
    "Input count mismatch.",
  );
  assert(
    tx.outputs.length > 0 && tx.outputs.length === tx.outputs_data.length,
    "Output count mismatch.",
  );
  assert(
    tx.witnesses.length === tx.inputs.length &&
      tx.witnesses[0] === placeholder() &&
      tx.witnesses.slice(1).every((w) => w === "0x"),
    "Unexpected signing witness.",
  );
  const before = new Map<string, bigint>();
  const after = new Map<string, bigint>();
  const seen = new Set<string>();
  let capacity = 0n;
  let outputCapacity = 0n;
  let recipientCapacity = 0n;
  const received = new Map<string, bigint>();
  const partial = request.amounts !== undefined;
  assert(
    !equalScript(owner, dest),
    "Destination must differ from this OmniLock wallet.",
  );
  const add = (map: Map<string, bigint>, o: Output, data: string) => {
    if (!o.type) {
      assert(data === "0x", "Unexpected plain-cell data.");
      return;
    }
    assert(
      o.type.code_hash === SUDT &&
        o.type.hash_type === "type" &&
        bytes(o.type.args).length === 32 &&
        bytes(data).length === 16,
      "Unsupported token cell.",
    );
    const key = token(o.type);
    assert(
      config.assets.some((a) => a.asset_id !== 1 && a.type_hash === key),
      "Unknown token in this transfer.",
    );
    map.set(key, (map.get(key) ?? 0n) + uint(data));
  };
  q.input_cells.forEach((c, i) => {
    const p = request.inputs[i];
    const key = `${p.tx_hash}:${p.index}`;
    assert(!seen.has(key), "Duplicate input.");
    seen.add(key);
    assert(
      same(c.out_point, p) &&
        same(tx.inputs[i].previous_output, p) &&
        tx.inputs[i].since === "0x0",
      "Input changed.",
    );
    assert(
      equalScript(c.output.lock, owner),
      "Cell belongs to another ETH address.",
    );
    assert(
      BigInt(c.output.capacity) >= minimum(c.output, c.output_data),
      "Invalid input capacity.",
    );
    capacity += BigInt(c.output.capacity);
    add(before, c.output, c.output_data);
  });
  let min = 0n;
  tx.outputs.forEach((o, i) => {
    assert(
      equalScript(o.lock, dest) || (partial && equalScript(o.lock, owner)),
      "The output destination or change owner was substituted.",
    );
    const required = minimum(o, tx.outputs_data[i]);
    assert(BigInt(o.capacity) >= required, "Destination cell needs more CKB.");
    if (equalScript(o.lock, dest)) {
      min += required;
      recipientCapacity += BigInt(o.capacity);
      add(received, o, tx.outputs_data[i]);
    }
    outputCapacity += BigInt(o.capacity);
    add(after, o, tx.outputs_data[i]);
  });
  assert(
    before.size === after.size &&
      [...before].every(([k, v]) => after.get(k) === v),
    "Token amounts changed.",
  );
  if (partial) {
    const selected = new Map<number, bigint>();
    for (const amount of request.amounts!) {
      assert(/^\d+$/.test(amount.raw), "Invalid partial amount.");
      assert(!selected.has(amount.asset_id), "Duplicate selected asset.");
      assert(
        config.assets.some((a) => a.asset_id === amount.asset_id),
        "Unknown selected asset.",
      );
      selected.set(amount.asset_id, BigInt(amount.raw));
    }
    assert(
      (selected.get(1) ?? 0n) > 0n && recipientCapacity === selected.get(1),
      "Requested CKB amount differs from recipient outputs.",
    );
    for (const a of config.assets.filter((a) => a.asset_id !== 1)) {
      const n = selected.get(a.asset_id) ?? 0n;
      assert(
        n <= (before.get(a.type_hash) ?? 0n),
        "Requested token amount exceeds remaining balance.",
      );
      assert(
        (received.get(a.type_hash) ?? 0n) === n,
        "Requested token amount differs from recipient outputs.",
      );
    }
  }
  const d = digest(tx);
  const expectedFee = (BigInt(d.size) * BigInt(config.fee_rate) + 999n) / 1000n;
  assert(
    expectedFee <= 1000000n &&
      expectedFee <= BigInt(config.max_fee_shannons ?? "1000000"),
    "Network fee exceeds 0.01 CKB.",
  );
  assert(
    config.fee_rate >= 1 &&
      config.fee_rate <= 100000 &&
      q.fee_rate === config.fee_rate &&
      d.size <= 512000,
    "Fee/size limit exceeded.",
  );
  assert(
    capacity - outputCapacity === expectedFee &&
      BigInt(q.fee_shannons) === expectedFee,
    "Network fee differs from the quoted fee.",
  );
  assert(
    BigInt(q.received_capacity_shannons) === recipientCapacity &&
      BigInt(q.minimum_capacity_shannons) === min,
    "Capacity summary differs from the transaction.",
  );
  assert(
    d.hash === q.transaction_hash && d.message === q.signing_message,
    "Signing digest does not match the reviewed transaction.",
  );
  return q;
}
export function verifyWalletSignature(
  message: string,
  signature: string,
  address: string,
) {
  assert(
    verifyMessage(getBytes(message), signature).toLowerCase() ===
      address.toLowerCase(),
    "The wallet signed with a different account.",
  );
}

export function transferTokens(q: Quote, config: Pick<Config, "assets">) {
  const tokens = new Map<number, bigint>();
  for (let i = 0; i < q.transaction.outputs.length; i++) {
    const output = q.transaction.outputs[i];
    const t = output.type;
    if (!t || !equalScript(output.lock, q.destination_lock)) continue;
    const asset = config.assets.find((a) => a.type_hash === token(t));
    assert(asset, "Unknown output asset");
    tokens.set(
      asset.asset_id,
      (tokens.get(asset.asset_id) ?? 0n) + uint(q.transaction.outputs_data[i]),
    );
  }
  return [...tokens].map(([asset_id, amount]) => ({
    asset: config.assets.find((a) => a.asset_id === asset_id)!,
    raw: amount.toString(),
  }));
}
