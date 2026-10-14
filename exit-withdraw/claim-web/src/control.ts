import { blockchain, utils } from "@ckb-lumos/base";
import { getBytes, hexlify, verifyMessage } from "ethers";
import { minimum, OMNI, rawTransaction } from "./verify";
import { verifyScope, type Protocol } from "./scope";
import type { Cell, Transaction } from "./types";
export type Group = { lock_hash: string; input_indices: number[]; kind: string; message: string; signing_digest: string; required_signatures: number; identities: string[] };
export type Approval = { transaction_hash: string; signatures: {lock_hash: string; signature: string}[] };
export type Proposal = { control_accounts: number[]; protocol: Protocol; plan_manifest: string; proposal: { protocol: Protocol; plan_manifest: string; skeleton: {transaction: Transaction; input_cells: Cell[]}; signatures: Approval[] }; signing_request: {transaction_hash: string; groups: Group[]} };
function check(v:unknown,m:string):asserts v { if(!v) throw Error(m); }
const scriptHash=(s:Cell["output"]["lock"])=>utils.computeScriptHash({codeHash:s.code_hash,hashType:s.hash_type,args:s.args});
function le(n:number){ const b=new Uint8Array(8);new DataView(b.buffer).setBigUint64(0,BigInt(n),true);return hexlify(b); }
export function verifyControlProposal(p:Proposal,protocol:Protocol,manifest:string,owner:string,id:string):Group {
  verifyScope(p,protocol,manifest); verifyScope(p.proposal,protocol,manifest);
  const sk=p.proposal.skeleton, tx=sk.transaction;
  const hash=utils.ckbHash(blockchain.RawTransaction.pack(rawTransaction(tx)));
  check(hash===id && hash===p.signing_request.transaction_hash,"Proposal transaction changed");
  check(tx.inputs.length===sk.input_cells.length,"Missing input resolution");
  for(let i=0;i<tx.inputs.length;i++) check(tx.inputs[i].previous_output.tx_hash===sk.input_cells[i].out_point.tx_hash && tx.inputs[i].previous_output.index===sk.input_cells[i].out_point.index,"Input outpoint mismatch");
  const lock={code_hash:OMNI,hash_type:"type" as const,args:`0x01${owner.slice(2).toLowerCase()}00`};
  const lockHash=scriptHash(lock);
  const groups=p.signing_request.groups.filter(g=>g.lock_hash===lockHash);
  check(groups.length===1,"Wallet is not an authorizing controller in this proposal");
  const group=groups[0];
  check(group.kind==="ethereum_personal_sign" && group.required_signatures===1 && group.identities.length===1 && group.identities[0]===owner.toLowerCase(),"Wrong authorization identity");
  const indices=sk.input_cells.flatMap((c,i)=>scriptHash(c.output.lock)===lockHash?[i]:[]);
  check(indices.length===1 && JSON.stringify(indices)===JSON.stringify(group.input_indices),"Duplicate or mismatched authorization inputs");
  const input=sk.input_cells[indices[0]];
  check(!input.output.type && input.output_data==="0x","Authorization requires a plain CKB cell");
  check(tx.outputs.some((o,i)=>scriptHash(o.lock)===lockHash && o.capacity===input.output.capacity && !o.type && tx.outputs_data[i]==="0x"),"Controller authorization cell is not returned unchanged");
  for(let i=0;i<tx.outputs.length;i++) check(BigInt(tx.outputs[i].capacity)>=minimum(tx.outputs[i],tx.outputs_data[i]),"Output below occupied capacity");
  const h=new utils.CKBHasher();h.update(hash);
  for(const i of [...indices,...tx.witnesses.slice(tx.inputs.length).map((_,i)=>i+tx.inputs.length)]) {
    let w=getBytes(tx.witnesses[i]);
    if(i===indices[0]){
      const parsed=blockchain.WitnessArgs.unpack(w);
      check(parsed.lock!==undefined && getBytes(parsed.lock).length===85,"Invalid OmniLock placeholder");
      w=blockchain.WitnessArgs.pack({...parsed,lock:hexlify(new Uint8Array(85))});
    }
    h.update(le(w.length));h.update(hexlify(w));
  }
  check(h.digestHex()===group.message,"Signing message does not match the transaction");
  return group;
}
export function mergeApprovals(p:Proposal,existing:Approval[],incoming:Approval[]):Approval[]{
  const seen=new Set<string>(); const all=[...existing,...incoming];
  for(const a of all){
    check(a.transaction_hash===p.signing_request.transaction_hash,"Approval belongs to a different transaction");
    for(const s of a.signatures){
      const g=p.signing_request.groups.find(g=>g.lock_hash===s.lock_hash);
      check(g && g.kind==="ethereum_personal_sign" && g.identities.length===1,"Unknown ETH approval group");
      check(!seen.has(s.lock_hash),"Duplicate controller approval");seen.add(s.lock_hash);
      check(verifyMessage(getBytes(g.message),s.signature).toLowerCase()===g.identities[0],"Approval is not from the expected controller");
    }
  }
  return all;
}
