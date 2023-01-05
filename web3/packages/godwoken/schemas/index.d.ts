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

export function SerializeUint32Vec(value: Array<CanCastToArrayBuffer>): ArrayBuffer;
export class Uint32Vec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Uint32;
  length(): number;
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

export function SerializeGlobalStateV0(value: object): ArrayBuffer;
export class GlobalStateV0 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getRollupConfigHash(): Byte32;
  getAccount(): AccountMerkleState;
  getBlock(): BlockMerkleState;
  getRevertedBlockRoot(): Byte32;
  getTipBlockHash(): Byte32;
  getLastFinalizedBlockNumber(): Uint64;
  getStatus(): number;
}

export function SerializeGlobalState(value: object): ArrayBuffer;
export class GlobalState {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getRollupConfigHash(): Byte32;
  getAccount(): AccountMerkleState;
  getBlock(): BlockMerkleState;
  getRevertedBlockRoot(): Byte32;
  getTipBlockHash(): Byte32;
  getTipBlockTimestamp(): Uint64;
  getLastFinalizedBlockNumber(): Uint64;
  getStatus(): number;
  getVersion(): number;
}

export function SerializeAllowedTypeHash(value: object): ArrayBuffer;
export class AllowedTypeHash {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getType(): number;
  getHash(): Byte32;
}

export function SerializeAllowedTypeHashVec(value: Array<object>): ArrayBuffer;
export class AllowedTypeHashVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): AllowedTypeHash;
  length(): number;
}

export function SerializeRollupConfig(value: object): ArrayBuffer;
export class RollupConfig {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getL1SudtScriptTypeHash(): Byte32;
  getCustodianScriptTypeHash(): Byte32;
  getDepositScriptTypeHash(): Byte32;
  getWithdrawalScriptTypeHash(): Byte32;
  getChallengeScriptTypeHash(): Byte32;
  getStakeScriptTypeHash(): Byte32;
  getL2SudtValidatorScriptTypeHash(): Byte32;
  getBurnLockHash(): Byte32;
  getRequiredStakingCapacity(): Uint64;
  getChallengeMaturityBlocks(): Uint64;
  getFinalityBlocks(): Uint64;
  getRewardBurnRate(): number;
  getChainId(): Uint64;
  getAllowedEoaTypeHashes(): AllowedTypeHashVec;
  getAllowedContractTypeHashes(): AllowedTypeHashVec;
}

export function SerializeRawL2Transaction(value: object): ArrayBuffer;
export class RawL2Transaction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getChainId(): Uint64;
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
  getSignature(): Bytes;
}

export function SerializeL2TransactionVec(value: Array<object>): ArrayBuffer;
export class L2TransactionVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): L2Transaction;
  length(): number;
}

export function SerializeSubmitTransactions(value: object): ArrayBuffer;
export class SubmitTransactions {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getTxWitnessRoot(): Byte32;
  getTxCount(): Uint32;
  getPrevStateCheckpoint(): Byte32;
}

export function SerializeSubmitWithdrawals(value: object): ArrayBuffer;
export class SubmitWithdrawals {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getWithdrawalWitnessRoot(): Byte32;
  getWithdrawalCount(): Uint32;
}

export function SerializeRawL2Block(value: object): ArrayBuffer;
export class RawL2Block {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getNumber(): Uint64;
  getBlockProducer(): Bytes;
  getParentBlockHash(): Byte32;
  getStakeCellOwnerLockHash(): Byte32;
  getTimestamp(): Uint64;
  getPrevAccount(): AccountMerkleState;
  getPostAccount(): AccountMerkleState;
  getStateCheckpointList(): Byte32Vec;
  getSubmitWithdrawals(): SubmitWithdrawals;
  getSubmitTransactions(): SubmitTransactions;
}

export function SerializeRawL2BlockVec(value: Array<object>): ArrayBuffer;
export class RawL2BlockVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): RawL2Block;
  length(): number;
}

export function SerializeL2Block(value: object): ArrayBuffer;
export class L2Block {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRaw(): RawL2Block;
  getKvState(): KVPairVec;
  getKvStateProof(): Bytes;
  getTransactions(): L2TransactionVec;
  getBlockProof(): Bytes;
  getWithdrawals(): WithdrawalRequestVec;
}

export function SerializeDepositRequest(value: object): ArrayBuffer;
export class DepositRequest {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getCapacity(): Uint64;
  getAmount(): Uint128;
  getSudtScriptHash(): Byte32;
  getScript(): Script;
  getRegistryId(): Uint32;
}

export function SerializeDepositRequestVec(value: Array<object>): ArrayBuffer;
export class DepositRequestVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): DepositRequest;
  length(): number;
}

export function SerializeRawWithdrawalRequest(value: object): ArrayBuffer;
export class RawWithdrawalRequest {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getNonce(): Uint32;
  getChainId(): Uint64;
  getCapacity(): Uint64;
  getAmount(): Uint128;
  getSudtScriptHash(): Byte32;
  getAccountScriptHash(): Byte32;
  getRegistryId(): Uint32;
  getOwnerLockHash(): Byte32;
  getFee(): Uint128;
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
  getRaw(): RawWithdrawalRequest;
  getSignature(): Bytes;
}

export function SerializeKVPair(value: object): ArrayBuffer;
export class KVPair {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
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
  getBlockProducer(): Bytes;
  getNumber(): Uint64;
  getTimestamp(): Uint64;
}

export function SerializeDepositLockArgs(value: object): ArrayBuffer;
export class DepositLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getOwnerLockHash(): Byte32;
  getLayer2Lock(): Script;
  getCancelTimeout(): Uint64;
  getRegistryId(): Uint32;
}

export function SerializeCustodianLockArgs(value: object): ArrayBuffer;
export class CustodianLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getDepositBlockHash(): Byte32;
  getDepositBlockNumber(): Uint64;
  getDepositLockArgs(): DepositLockArgs;
}

export function SerializeUnlockCustodianViaRevertWitness(value: object): ArrayBuffer;
export class UnlockCustodianViaRevertWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getDepositLockHash(): Byte32;
}

export function SerializeWithdrawalLockArgs(value: object): ArrayBuffer;
export class WithdrawalLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getWithdrawalBlockHash(): Byte32;
  getWithdrawalBlockNumber(): Uint64;
  getAccountScriptHash(): Byte32;
  getOwnerLockHash(): Byte32;
}

export function SerializeUnlockWithdrawalWitness(value: UnionType): ArrayBuffer;
export class UnlockWithdrawalWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeUnlockWithdrawalViaFinalize(value: object): ArrayBuffer;
export class UnlockWithdrawalViaFinalize {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
}

export function SerializeUnlockWithdrawalViaRevert(value: object): ArrayBuffer;
export class UnlockWithdrawalViaRevert {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getCustodianLockHash(): Byte32;
}

export function SerializeStakeLockArgs(value: object): ArrayBuffer;
export class StakeLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getOwnerLockHash(): Byte32;
  getStakeBlockNumber(): Uint64;
}

export function SerializeMetaContractArgs(value: UnionType): ArrayBuffer;
export class MetaContractArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeFee(value: object): ArrayBuffer;
export class Fee {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getRegistryId(): Uint32;
  getAmount(): Uint128;
}

export function SerializeCreateAccount(value: object): ArrayBuffer;
export class CreateAccount {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getScript(): Script;
  getFee(): Fee;
}

export function SerializeBatchCreateEthAccounts(value: object): ArrayBuffer;
export class BatchCreateEthAccounts {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getScripts(): ScriptVec;
  getFee(): Fee;
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
  getAddress(): Bytes;
}

export function SerializeSUDTTransfer(value: object): ArrayBuffer;
export class SUDTTransfer {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getToAddress(): Bytes;
  getAmount(): Uint256;
  getFee(): Fee;
}

export function SerializeChallengeTarget(value: object): ArrayBuffer;
export class ChallengeTarget {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getBlockHash(): Byte32;
  getTargetIndex(): Uint32;
  getTargetType(): number;
}

export function SerializeChallengeLockArgs(value: object): ArrayBuffer;
export class ChallengeLockArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getTarget(): ChallengeTarget;
  getRewardsReceiverLock(): Script;
}

export function SerializeChallengeWitness(value: object): ArrayBuffer;
export class ChallengeWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRawL2Block(): RawL2Block;
  getBlockProof(): Bytes;
}

export function SerializeScriptVec(value: Array<object>): ArrayBuffer;
export class ScriptVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): Script;
  length(): number;
}

export function SerializeBlockHashEntry(value: object): ArrayBuffer;
export class BlockHashEntry {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getNumber(): Uint64;
  getHash(): Byte32;
}

export function SerializeBlockHashEntryVec(value: Array<object>): ArrayBuffer;
export class BlockHashEntryVec {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): BlockHashEntry;
  length(): number;
}

export function SerializeCKBMerkleProof(value: object): ArrayBuffer;
export class CKBMerkleProof {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getIndices(): Uint32Vec;
  getLemmas(): Byte32Vec;
}

export function SerializeCCTransactionWitness(value: object): ArrayBuffer;
export class CCTransactionWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getL2Tx(): L2Transaction;
  getRawL2Block(): RawL2Block;
  getTxProof(): CKBMerkleProof;
  getKvStateProof(): Bytes;
  getBlockHashesProof(): Bytes;
  getAccountCount(): Uint32;
  getKvState(): KVPairVec;
  getLoadData(): BytesVec;
  getScripts(): ScriptVec;
  getReturnDataHash(): Byte32;
  getBlockHashes(): BlockHashEntryVec;
}

export function SerializeCCTransactionSignatureWitness(value: object): ArrayBuffer;
export class CCTransactionSignatureWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRawL2Block(): RawL2Block;
  getL2Tx(): L2Transaction;
  getTxProof(): CKBMerkleProof;
  getKvState(): KVPairVec;
  getKvStateProof(): Bytes;
  getAccountCount(): Uint32;
  getSender(): Script;
  getReceiver(): Script;
}

export function SerializeCCWithdrawalWitness(value: object): ArrayBuffer;
export class CCWithdrawalWitness {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRawL2Block(): RawL2Block;
  getWithdrawal(): WithdrawalRequest;
  getSender(): Script;
  getOwnerLock(): Script;
  getWithdrawalProof(): CKBMerkleProof;
  getKvStateProof(): Bytes;
  getKvState(): KVPairVec;
  getAccountCount(): Uint32;
}

export function SerializeRollupSubmitBlock(value: object): ArrayBuffer;
export class RollupSubmitBlock {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getBlock(): L2Block;
  getRevertedBlockHashes(): Byte32Vec;
  getRevertedBlockProof(): Bytes;
}

export function SerializeRollupEnterChallenge(value: object): ArrayBuffer;
export class RollupEnterChallenge {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getWitness(): ChallengeWitness;
}

export function SerializeRollupCancelChallenge(value: object): ArrayBuffer;
export class RollupCancelChallenge {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
}

export function SerializeRollupRevert(value: object): ArrayBuffer;
export class RollupRevert {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getRevertedBlocks(): RawL2BlockVec;
  getBlockProof(): Bytes;
  getRevertedBlockProof(): Bytes;
  getNewTipBlock(): RawL2Block;
}

export function SerializeRollupAction(value: UnionType): ArrayBuffer;
export class RollupAction {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeByte20(value: CanCastToArrayBuffer): ArrayBuffer;
export class Byte20 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  static size(): Number;
}

export function SerializeETHAddrRegArgs(value: UnionType): ArrayBuffer;
export class ETHAddrRegArgs {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  unionType(): string;
  value(): any;
}

export function SerializeEthToGw(value: object): ArrayBuffer;
export class EthToGw {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getEthAddress(): Byte20;
}

export function SerializeGwToEth(value: object): ArrayBuffer;
export class GwToEth {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getGwScriptHash(): Byte32;
}

export function SerializeSetMapping(value: object): ArrayBuffer;
export class SetMapping {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  static size(): Number;
  getGwScriptHash(): Byte32;
  getFee(): Fee;
}

export function SerializeBatchSetMapping(value: object): ArrayBuffer;
export class BatchSetMapping {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getGwScriptHashes(): Byte32Vec;
  getFee(): Fee;
}

export function SerializeUint16(value: CanCastToArrayBuffer): ArrayBuffer;
export class Uint16 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  indexAt(i: number): number;
  raw(): ArrayBuffer;
  toBigEndianUint16(): number;
  toLittleEndianUint16(): number;
  static size(): Number;
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
  getExtraHash(): Byte32;
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

export function SerializeBlockV1(value: object): ArrayBuffer;
export class BlockV1 {
  constructor(reader: CanCastToArrayBuffer, options?: CreateOptions);
  validate(compatible?: boolean): void;
  getHeader(): Header;
  getUncles(): UncleBlockVec;
  getTransactions(): TransactionVec;
  getProposals(): ProposalShortIdVec;
  getExtension(): Bytes;
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

