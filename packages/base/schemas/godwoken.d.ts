export interface CastToArrayBuffer {
  toArrayBuffer(): ArrayBuffer;
}

export type CanCastToArrayBuffer = ArrayBuffer | CastToArrayBuffer;

export interface CreateOptions {
  validate?: boolean;
}

export interface UnionType {
  type: string;
  value: any;
}

export function SerializeByte32Opt(value: CanCastToArrayBuffer | null): ArrayBuffer;
export class Byte32Opt {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  value(): Byte32;
  hasValue(): boolean;
}

export function SerializeByte20(value: CanCastToArrayBuffer): ArrayBuffer;
export class Byte20 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeSignature(value: CanCastToArrayBuffer): ArrayBuffer;
export class Signature {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeBlockMerkleState(value: object): ArrayBuffer;
export class BlockMerkleState {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getMerkleRoot(): Byte32;
  getCount(): Uint64;
}

export function SerializeAccountMerkleState(value: object): ArrayBuffer;
export class AccountMerkleState {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getMerkleRoot(): Byte32;
  getCount(): Uint32;
}

export function SerializeGlobalState(value: object): ArrayBuffer;
export class GlobalState {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getAccount(): AccountMerkleState;
  getBlock(): BlockMerkleState;
  getRevertedBlockRoot(): Byte32;
  getLastFinalizedBlockNumber(): Uint64;
  getStatus(): Status;
}

export function SerializeStatus(value: UnionType): ArrayBuffer;
export class Status {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeRunning(value: object): ArrayBuffer;
export class Running {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
}

export function SerializeReverting(value: object): ArrayBuffer;
export class Reverting {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getNextBlockNumber(): Uint64;
  getChallengerId(): Uint32;
}

export function SerializeRawL2Transaction(value: object): ArrayBuffer;
export class RawL2Transaction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getFromId(): Uint32;
  getToId(): Uint32;
  getNonce(): Uint32;
  getArgs(): Bytes;
}

export function SerializeL2Transaction(value: object): ArrayBuffer;
export class L2Transaction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRaw(): RawL2Transaction;
  getSignature(): Signature;
}

export function SerializeL2TransactionVec(value: Array<object>): ArrayBuffer;
export class L2TransactionVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): L2Transaction;
  length(): number;
}

export function SerializeRawL2Block(value: object): ArrayBuffer;
export class RawL2Block {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getNumber(): Uint64;
  getAggregatorId(): Uint32;
  getStakeCellOwnerLockHash(): Byte32;
  getTimestamp(): Uint64;
  getPrevAccount(): AccountMerkleState;
  getPostAccount(): AccountMerkleState;
  getSubmitTransactions(): SubmitTransactions;
  getWithdrawalRequestsRoot(): Byte32;
}

export function SerializeL2Block(value: object): ArrayBuffer;
export class L2Block {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRaw(): RawL2Block;
  getSignature(): Signature;
  getKvState(): KVPairVec;
  getKvStateProof(): Bytes;
  getTransactions(): L2TransactionVec;
  getBlockProof(): Bytes;
  getWithdrawalRequests(): WithdrawalRequestVec;
}

export function SerializeDepositionRequest(value: object): ArrayBuffer;
export class DepositionRequest {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getCapacity(): Uint64;
  getAmount(): Uint128;
  getSudtScript(): Script;
  getScript(): Script;
}

export function SerializeRawWithdrawalRequest(value: object): ArrayBuffer;
export class RawWithdrawalRequest {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getNonce(): Uint32;
  getCapacity(): Uint64;
  getAmount(): Uint128;
  getSudtScriptHash(): Byte32;
  getAccountScriptHash(): Byte32;
  getSellAmount(): Uint128;
  getSellCapacity(): Uint64;
  getOwnerLockHash(): Byte32;
  getPaymentLockHash(): Byte32;
}

export function SerializeWithdrawalRequestVec(value: Array<object>): ArrayBuffer;
export class WithdrawalRequestVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): WithdrawalRequest;
  length(): number;
}

export function SerializeWithdrawalRequest(value: object): ArrayBuffer;
export class WithdrawalRequest {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getRaw(): RawWithdrawalRequest;
  getSignature(): Signature;
}

export function SerializeSubmitTransactions(value: object): ArrayBuffer;
export class SubmitTransactions {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getTxWitnessRoot(): Byte32;
  getTxCount(): Uint32;
  getCompactedPostRootList(): Byte32Vec;
}

export function SerializeKVPair(value: object): ArrayBuffer;
export class KVPair {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getK(): Byte32;
  getV(): Byte32;
}

export function SerializeKVPairVec(value: Array<object>): ArrayBuffer;
export class KVPairVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): KVPair;
  length(): number;
}

export function SerializeBlockInfo(value: object): ArrayBuffer;
export class BlockInfo {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getAggregatorId(): Uint32;
  getNumber(): Uint64;
  getTimestamp(): Uint64;
}

export function SerializeDepositionLockArgs(value: object): ArrayBuffer;
export class DepositionLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getOwnerLockHash(): Byte32;
  getLayer2Lock(): Script;
  getCancelTimeout(): Uint64;
}

export function SerializeCustodianLockArgs(value: object): ArrayBuffer;
export class CustodianLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getDepositionLockArgs(): DepositionLockArgs;
  getDepositionBlockHash(): Byte32;
  getDepositionBlockNumber(): Uint64;
}

export function SerializeUnlockCustodianViaRevert(value: object): ArrayBuffer;
export class UnlockCustodianViaRevert {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getBlockProof(): Bytes;
  getDepositionLockHash(): Byte32;
}

export function SerializeWithdrawalLockArgs(value: object): ArrayBuffer;
export class WithdrawalLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getDepositionBlockHash(): Byte32;
  getDepositionBlockNumber(): Uint64;
  getWithdrawalBlockHash(): Byte32;
  getWithdrawalBlockNumber(): Uint64;
  getSudtScriptHash(): Byte32;
  getSellAmount(): Uint128;
  getSellCapacity(): Uint64;
  getOwnerLockHash(): Byte32;
  getPaymentLockHash(): Byte32;
}

export function SerializeUnlockWithdrawal(value: UnionType): ArrayBuffer;
export class UnlockWithdrawal {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeUnlockWithdrawalViaFinalize(value: object): ArrayBuffer;
export class UnlockWithdrawalViaFinalize {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getBlockProof(): Bytes;
}

export function SerializeUnlockWithdrawalViaRevert(value: object): ArrayBuffer;
export class UnlockWithdrawalViaRevert {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getBlockProof(): Bytes;
  getCustodianLockHash(): Byte32;
}

export function SerializeUnlockWithdrawalViaTrade(value: object): ArrayBuffer;
export class UnlockWithdrawalViaTrade {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getOwnerLock(): Script;
}

export function SerializeStakeLockArgs(value: object): ArrayBuffer;
export class StakeLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getOwnerLockHash(): Byte32;
  getSigningPubkeyHash(): Byte20;
  getStakeBlockNumber(): Uint64;
}

export function SerializeMetaContractArgs(value: UnionType): ArrayBuffer;
export class MetaContractArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeCreateAccount(value: object): ArrayBuffer;
export class CreateAccount {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getScript(): Script;
}

export function SerializeSUDTArgs(value: UnionType): ArrayBuffer;
export class SUDTArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeSUDTQuery(value: object): ArrayBuffer;
export class SUDTQuery {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getAccountId(): Uint32;
}

export function SerializeSUDTTransfer(value: object): ArrayBuffer;
export class SUDTTransfer {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getTo(): Uint32;
  getAmount(): Uint128;
  getFee(): Uint128;
}

export function SerializeStartChallenge(value: object): ArrayBuffer;
export class StartChallenge {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getBlockHash(): Byte32;
  getBlockNumber(): Uint64;
  getTxIndex(): Uint32;
}

export function SerializeScriptVec(value: Array<object>): ArrayBuffer;
export class ScriptVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Script;
  length(): number;
}

export function SerializeCancelChallenge(value: object): ArrayBuffer;
export class CancelChallenge {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRawL2Block(): RawL2Block;
  getL2Tx(): L2Transaction;
  getKvState(): KVPairVec;
  getKvStateProof(): Bytes;
  getScripts(): ScriptVec;
  getReturnDataHash(): Byte32;
  getTxProof(): Bytes;
  getBlockProof(): Bytes;
}

export function SerializeHeaderInfo(value: object): ArrayBuffer;
export class HeaderInfo {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getNumber(): Uint64;
  getBlockHash(): Byte32;
}

export function SerializeUint32(value: CanCastToArrayBuffer): ArrayBuffer;
export class Uint32 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  toBigEndianUint32(): number;
  toLittleEndianUint32(): number;
  static size(): Number;
}

export function SerializeUint64(value: CanCastToArrayBuffer): ArrayBuffer;
export class Uint64 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  toBigEndianBigUint64(): bigint;
  toLittleEndianBigUint64(): bigint;
  static size(): Number;
}

export function SerializeUint128(value: CanCastToArrayBuffer): ArrayBuffer;
export class Uint128 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeByte32(value: CanCastToArrayBuffer): ArrayBuffer;
export class Byte32 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeUint256(value: CanCastToArrayBuffer): ArrayBuffer;
export class Uint256 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeBytes(value: CanCastToArrayBuffer): ArrayBuffer;
export class Bytes {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  length(): number;
}

export function SerializeBytesOpt(value: CanCastToArrayBuffer | null): ArrayBuffer;
export class BytesOpt {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  value(): Bytes;
  hasValue(): boolean;
}

export function SerializeBytesVec(value: Array<CanCastToArrayBuffer>): ArrayBuffer;
export class BytesVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Bytes;
  length(): number;
}

export function SerializeByte32Vec(value: Array<CanCastToArrayBuffer>): ArrayBuffer;
export class Byte32Vec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Byte32;
  length(): number;
}

export function SerializeScriptOpt(value: object | null): ArrayBuffer;
export class ScriptOpt {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  value(): Script;
  hasValue(): boolean;
}

export function SerializeProposalShortId(value: CanCastToArrayBuffer): ArrayBuffer;
export class ProposalShortId {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeUncleBlockVec(value: Array<object>): ArrayBuffer;
export class UncleBlockVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): UncleBlock;
  length(): number;
}

export function SerializeTransactionVec(value: Array<object>): ArrayBuffer;
export class TransactionVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Transaction;
  length(): number;
}

export function SerializeProposalShortIdVec(value: Array<CanCastToArrayBuffer>): ArrayBuffer;
export class ProposalShortIdVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): ProposalShortId;
  length(): number;
}

export function SerializeCellDepVec(value: Array<object>): ArrayBuffer;
export class CellDepVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): CellDep;
  length(): number;
}

export function SerializeCellInputVec(value: Array<object>): ArrayBuffer;
export class CellInputVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): CellInput;
  length(): number;
}

export function SerializeCellOutputVec(value: Array<object>): ArrayBuffer;
export class CellOutputVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): CellOutput;
  length(): number;
}

export function SerializeScript(value: object): ArrayBuffer;
export class Script {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getCodeHash(): Byte32;
  getHashType(): number;
  getArgs(): Bytes;
}

export function SerializeOutPoint(value: object): ArrayBuffer;
export class OutPoint {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getTxHash(): Byte32;
  getIndex(): Uint32;
}

export function SerializeCellInput(value: object): ArrayBuffer;
export class CellInput {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getSince(): Uint64;
  getPreviousOutput(): OutPoint;
}

export function SerializeCellOutput(value: object): ArrayBuffer;
export class CellOutput {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getCapacity(): Uint64;
  getLock(): Script;
  getType(): ScriptOpt;
}

export function SerializeCellDep(value: object): ArrayBuffer;
export class CellDep {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getOutPoint(): OutPoint;
  getDepType(): number;
}

export function SerializeRawTransaction(value: object): ArrayBuffer;
export class RawTransaction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getVersion(): Uint32;
  getCellDeps(): CellDepVec;
  getHeaderDeps(): Byte32Vec;
  getInputs(): CellInputVec;
  getOutputs(): CellOutputVec;
  getOutputsData(): BytesVec;
}

export function SerializeTransaction(value: object): ArrayBuffer;
export class Transaction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRaw(): RawTransaction;
  getWitnesses(): BytesVec;
}

export function SerializeRawHeader(value: object): ArrayBuffer;
export class RawHeader {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getVersion(): Uint32;
  getCompactTarget(): Uint32;
  getTimestamp(): Uint64;
  getNumber(): Uint64;
  getEpoch(): Uint64;
  getParentHash(): Byte32;
  getTransactionsRoot(): Byte32;
  getProposalsHash(): Byte32;
  getUnclesHash(): Byte32;
  getDao(): Byte32;
}

export function SerializeHeader(value: object): ArrayBuffer;
export class Header {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getRaw(): RawHeader;
  getNonce(): Uint128;
}

export function SerializeUncleBlock(value: object): ArrayBuffer;
export class UncleBlock {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getHeader(): Header;
  getProposals(): ProposalShortIdVec;
}

export function SerializeBlock(value: object): ArrayBuffer;
export class Block {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getHeader(): Header;
  getUncles(): UncleBlockVec;
  getTransactions(): TransactionVec;
  getProposals(): ProposalShortIdVec;
}

export function SerializeCellbaseWitness(value: object): ArrayBuffer;
export class CellbaseWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getLock(): Script;
  getMessage(): Bytes;
}

export function SerializeWitnessArgs(value: object): ArrayBuffer;
export class WitnessArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getLock(): BytesOpt;
  getInputType(): BytesOpt;
  getOutputType(): BytesOpt;
}

