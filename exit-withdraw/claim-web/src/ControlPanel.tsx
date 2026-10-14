import React,{useEffect,useRef,useState} from "react";
import {formatUnits} from "ethers";
import {utils} from "@ckb-lumos/base";
import {encodeToAddress} from "@ckb-lumos/helpers";
import {predefined} from "@ckb-lumos/config-manager";
import {endpoint,verifyScope,type ScopedConfig} from "./scope";
import {current} from "./wallet";
import type {Wallet} from "./types";
import {verifyControlProposal,mergeApprovals,type Approval,type Proposal} from "./control";
type Account={account_id:number;threshold:number;owners:string[];entitlements:{asset_id:number;raw:string}[];explanations:{asset_id:number;symbol:string;status:string;reason:string;calculated_entitlement:string}[]};
async function api<T>(url:string,body?:unknown):Promise<T>{const r=await fetch(url,{method:body?"POST":"GET",headers:body?{"Content-Type":"application/json"}:undefined,body:body?JSON.stringify(body):undefined});const v=await r.json();if(!r.ok)throw Error(v.error??"Request failed");return v;}
function download(value:unknown,name:string){const u=URL.createObjectURL(new Blob([JSON.stringify(value,null,2)],{type:"application/json"}));const a=document.createElement("a");a.href=u;a.download=name;a.click();URL.revokeObjectURL(u);}
export function ControlPanel({config,address,wallet,onBusy}:{config:ScopedConfig;address:string;wallet?:Wallet;onBusy:(s:string)=>void}){
 const [accounts,setAccounts]=useState<Account[]>([]),[selected,setSelected]=useState("");
 const [id,setId]=useState(""),[proposal,setProposal]=useState<Proposal>(),[approvals,setApprovals]=useState<Approval[]>([]),[error,setError]=useState(""),[status,setStatus]=useState("");
 const [submitted,setSubmitted]=useState("");
 const [working,setWorking]=useState(false);
 const alive=useRef(true);const busy=useRef(false);const context=`${config.protocol}:${config.plan_manifest}:${address}`;
 useEffect(()=>{alive.current=true;let canceled=false;api<{protocol:ScopedConfig["protocol"];plan_manifest:string;accounts:Account[]}>(endpoint(config.protocol,"controls")).then(v=>{verifyScope(v,config.protocol,config.plan_manifest);if(!canceled)setAccounts(v.accounts);}).catch(e=>{if(!canceled)setError(e.message)});return()=>{canceled=true;alive.current=false;};},[context]);
 const account=accounts.find(a=>String(a.account_id)===selected);
 async function work(label:string,f:()=>Promise<void>){if(busy.current)return;busy.current=true;setWorking(true);onBusy(label);setError("");try{await f()}catch(e){if(alive.current)setError((e as Error).message)}finally{busy.current=false;if(alive.current)setWorking(false);onBusy("")}}
 async function load(){await work("Loading contract proposal…",async()=>{if(!/^0x[0-9a-f]{64}$/.test(id))throw Error("Enter a transaction hash");const p=await api<Proposal>(endpoint(config.protocol,`control-proposal/${id}`));verifyControlProposal(p,config.protocol,config.plan_manifest,address,id);if(!p.control_accounts.includes(Number(selected)))throw Error("Proposal belongs to another contract account");if(alive.current){setProposal(p);setApprovals([]);setStatus("");}})}
 async function sign(){await work("Review and sign contract refund…",async()=>{if(!wallet||!proposal||!account)throw Error("Select an account and connect a controller wallet");if(!account.owners.includes(address.toLowerCase()))throw Error("Wallet is not a controller of this account");if(await current(wallet.provider)!==address)throw Error("Wallet changed");const g=verifyControlProposal(proposal,config.protocol,config.plan_manifest,address,id);const signature=await wallet.provider.request({method:"personal_sign",params:[g.message,address]});if(!alive.current)return;if(typeof signature!=="string"||await current(wallet.provider)!==address)throw Error("Wallet changed during signing");const next=mergeApprovals(proposal,approvals,[{transaction_hash:id,signatures:[{lock_hash:g.lock_hash,signature}]}]);setApprovals(next);download(next,`${config.protocol}-${id}-approvals.json`);})}
 async function submit(){await work("Validating and submitting on CKB…",async()=>{if(!proposal)throw Error("Load a proposal");verifyControlProposal(proposal,config.protocol,config.plan_manifest,address,id);const key=`gw-control-pending:${context}`;if(localStorage.getItem(key)===id)throw Error("Check the existing transaction before any retry");localStorage.setItem(key,id);setSubmitted(id);setStatus(`Submission pending: ${id}. Check this transaction before retrying.`);const r=await api<{transaction_hash:string}>(endpoint(config.protocol,"control-submit"),{protocol:config.protocol,plan_manifest:config.plan_manifest,transaction_hash:id,signatures:approvals});if(alive.current)setStatus(`Submitted: ${r.transaction_hash}`);})}
 function outputAsset(o:Proposal["proposal"]["skeleton"]["transaction"]["outputs"][number], data:string){
  if(!o.type)return "CKB only";
  const typeHash=utils.computeScriptHash({codeHash:o.type.code_hash,hashType:o.type.hash_type,args:o.type.args});
  const asset=config.assets.find(a=>a.type_hash===typeHash);
  if(asset && /^0x[0-9a-f]{32}$/i.test(data)){
   const raw=BigInt(`0x${data.slice(2).match(/../g)!.reverse().join("")}`);
   return `${asset.symbol??`Asset ${asset.asset_id}`}: ${asset.l1_decimals==null?`${raw} raw units`:formatUnits(raw,asset.l1_decimals)}`;
  }
  return `State / typed output (${(data.length-2)/2} bytes)`;
 }
 return <section className="card control-panel" aria-label="Contract account refunds">
 <h2>Contract Account</h2><p>Each controller approves the same destination and assets. Authorization CKB is returned unchanged. Ops prepares the proposal; no governance key is stored here.</p>
 <label>Contract account <select disabled={working} value={selected} onChange={e=>{setSelected(e.target.value);setProposal(undefined);setApprovals([]);}}><option value="">Choose an account</option>{accounts.map(a=><option key={a.account_id} value={a.account_id}>{a.account_id} — {a.threshold} of {a.owners.length}</option>)}</select></label>
 {account&&<><p>Required: {account.threshold} distinct controllers</p><ul>{account.owners.map(o=><li key={o}><code>{o}</code></li>)}</ul><table><thead><tr><th>Asset</th><th>Approved entitlement</th><th>Treatment</th></tr></thead><tbody>{account.explanations?.map((a,i)=><tr key={i}><td>{a.symbol??a.asset_id}</td><td>{a.calculated_entitlement}</td><td>{a.status}: {a.reason}</td></tr>)}</tbody></table></>}
 <p>Amounts above are total approved rights. A live proposal specifies this payment; it is checked against cumulative paid state.</p>
 <label>Proposal transaction hash <input disabled={working} value={id} onChange={e=>{setId(e.target.value);setProposal(undefined);setApprovals([])}} placeholder="0x…"/></label><button disabled={working||!address||!account} onClick={()=>void load()}>Load proposal</button>
 {proposal&&<><h3>Review every transaction output</h3><p>Transaction: <code>{id}</code></p><div style={{overflowX:"auto"}}><table className="control-outputs"><thead><tr><th>Output</th><th>CKB</th><th>Destination</th><th>Asset / data</th></tr></thead><tbody>{proposal.proposal.skeleton.transaction.outputs.map((o,i)=><tr key={i}><td>{i}</td><td>{formatUnits(BigInt(o.capacity),8)}</td><td style={{overflowWrap:"anywhere"}}>{encodeToAddress({codeHash:o.lock.code_hash,hashType:o.lock.hash_type,args:o.lock.args},{config:predefined.LINA})}</td><td style={{overflowWrap:"anywhere"}}>{outputAsset(o,proposal.proposal.skeleton.transaction.outputs_data[i])}{o.type&&<details><summary>Inspect script and raw data</summary><code>{o.type.code_hash} / {o.type.args}: {proposal.proposal.skeleton.transaction.outputs_data[i]}</code></details>}</td></tr>)}</tbody></table></div><p>{approvals.reduce((n,a)=>n+a.signatures.length,0)} distinct ETH approvals collected. Every authorization input must be signed.</p>
 <button disabled={working} onClick={()=>void sign()}>Sign and download approval</button><button onClick={()=>download(approvals,`${id}-approvals.json`)}>Export collected approvals</button>
 <label>Import co-signer approvals <input disabled={working} type="file" accept="application/json" onChange={e=>{const f=e.target.files?.[0];if(f)void work("Checking approvals…",async()=>{if(f.size>1024*1024)throw Error("File too large");const incoming=JSON.parse(await f.text());const next=mergeApprovals(proposal,approvals,incoming);if(alive.current)setApprovals(next)});e.target.value="";}}/></label>
 <button disabled={working || !config.broadcast_enabled || submitted===id || localStorage.getItem(`gw-control-pending:${context}`)===id} onClick={()=>void submit()}>Submit jointly signed refund</button></>}
 {error&&<p role="alert">{error}</p>}{status&&<p role="status">{status}</p>}
 {localStorage.getItem(`gw-control-pending:${context}`)&&<p>Last submitted or uncertain transaction: <code>{localStorage.getItem(`gw-control-pending:${context}`)}</code> <button onClick={()=>void work("Checking transaction…",async()=>{const hash=localStorage.getItem(`gw-control-pending:${context}`);const r=await api<{status:string}>(endpoint(config.protocol,`transaction/${hash}`));if(alive.current)setStatus(`Transaction ${hash}: ${r.status??"unknown; retain this proposal and contact Ops"}`)})}>Check transaction</button></p>}
 </section>
}
