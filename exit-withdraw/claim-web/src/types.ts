export type Script = {
  code_hash: string;
  hash_type: "data" | "type" | "data1" | "data2";
  args: string;
};
export type Point = { tx_hash: string; index: string };
export type Output = { capacity: string; lock: Script; type: Script | null };
export type Cell = {
  out_point: Point;
  output: Output;
  output_data: string;
  block_number: string;
  tx_index: string;
};
export type Dependency = {
  out_point: Point;
  dep_type: "code";
  data_hash: string;
};
export type Asset = {
  asset_id: number;
  symbol: string | null;
  decimals: number | null;
  l1_decimals: number | null;
  type_hash: string;
};
export type Config = {
  network: string;
  scope: string;
  assets: Asset[];
  dependencies: Dependency[];
  broadcast_enabled: boolean;
  fee_rate: number;
  plan_manifest: string;
  snapshot_root: string;
  max_fee_shannons?: string;
};
export type Balance = { asset_id: number; raw: string };
export type Explanation = {
  asset_id: number;
  symbol: string | null;
  calculated_entitlement: string;
  status: string;
  reason: string;
  protocols: string[];
};
export type Inventory = {
  unattributed_cells?: number;
  address: string;
  omnilock_address: string;
  entitlements: Balance[];
  available: Balance[];
  cells: Cell[];
  explanations: Explanation[];
  more_batches: boolean;
  unsupported_cells: number;
  confirming_cells: number;
  confirmations_required: number;
  observed_block: number;
};
export type WalletInventory = {
  address: string;
  omnilock_address: string;
  explorer_url: string;
  assets: Asset[];
  balances: Balance[];
  transferable: Balance[];
  available: Balance[];
  cells: Cell[];
  other_cells: Cell[];
  more_batches: boolean;
  confirming_cells: number;
  confirmations_required: number;
};
export type WalletHistory = {
  transactions: {
    tx_hash: string;
    block_number: string;
    cells: ["input" | "output", number][];
  }[];
  last_cursor: string;
  more: boolean;
};
export type BuildRequest = {
  address: string;
  destination: string;
  inputs: Point[];
  amounts?: Balance[];
};
export type Transaction = {
  version: string;
  cell_deps: { out_point: Point; dep_type: "code" }[];
  header_deps: string[];
  inputs: { since: string; previous_output: Point }[];
  outputs: Output[];
  outputs_data: string[];
  witnesses: string[];
};
export type Quote = {
  address: string;
  destination: string;
  destination_lock: Script;
  input_cells: Cell[];
  transaction: Transaction;
  transaction_hash: string;
  signing_message: string;
  fee_shannons: string;
  fee_rate: number;
  received_capacity_shannons: string;
  minimum_capacity_shannons: string;
};
export type Provider = {
  request(args: { method: string; params?: unknown[] }): Promise<unknown>;
  on?(event: string, callback: (arg: unknown) => void): void;
  removeListener?(event: string, callback: (arg: unknown) => void): void;
  providers?: Provider[];
  isMetaMask?: boolean;
  isRabby?: boolean;
  isCoinbaseWallet?: boolean;
};
export type Wallet = { id: string; name: string; provider: Provider };
declare global {
  interface Window {
    ethereum?: Provider;
  }
}
