import {describe,it,expect} from "vitest";
import {verifyControlProposal,mergeApprovals,type Proposal,type Approval} from "../src/control";
import v1 from "./control-v1-fixture.json";
import v0 from "./control-v0-fixture.json";
for(const [protocol,f] of [["v1",v1],["v0",v0]] as const){describe(`${protocol} real VM-validated control proposal`,()=>{
 const p=f.response as unknown as Proposal;const id=p.signing_request.transaction_hash;
 it("reconstructs the exact OmniLock message and accepts two independent approvals",()=>{expect(verifyControlProposal(p,protocol,p.plan_manifest,f.owner,id).message).toBeTruthy();expect(mergeApprovals(p,[],f.approvals as Approval[])).toHaveLength(2);});
 it("rejects cross-protocol, cross-release and unrelated wallets",()=>{expect(()=>verifyControlProposal(p,protocol==="v1"?"v0":"v1",p.plan_manifest,f.owner,id)).toThrow();expect(()=>verifyControlProposal(p,protocol,"wrong",f.owner,id)).toThrow();expect(()=>verifyControlProposal(p,protocol,p.plan_manifest,f.owners[2],id)).toThrow();});
 it("rejects altered destination, amounts or transaction hash",()=>{for(const change of [(q:Proposal)=>{q.proposal.skeleton.transaction.outputs[1].lock.args="0x"+"12".repeat(22)},(q:Proposal)=>{q.proposal.skeleton.transaction.outputs[1].capacity="0x123"},(q:Proposal)=>{q.signing_request.transaction_hash="0x"+"00".repeat(32)}]){const q=structuredClone(p);change(q);expect(()=>verifyControlProposal(q,protocol,p.plan_manifest,f.owner,id)).toThrow();}});
 it("rejects modified signing digest and authorization input list",()=>{const q=structuredClone(p);const g=q.signing_request.groups.find(g=>g.identities.includes(f.owner))!;g.message="0x"+"00".repeat(32);expect(()=>verifyControlProposal(q,protocol,p.plan_manifest,f.owner,id)).toThrow();g.input_indices=[];expect(()=>verifyControlProposal(q,protocol,p.plan_manifest,f.owner,id)).toThrow();});
 it("rejects duplicate, outside and cross-transaction approvals",()=>{const approvals=f.approvals as Approval[];expect(()=>mergeApprovals(p,approvals,[approvals[0]])).toThrow();const bad=structuredClone(approvals);bad[0].transaction_hash="0x"+"aa".repeat(32);expect(()=>mergeApprovals(p,[],bad)).toThrow();const wrong=structuredClone(approvals);wrong[0].signatures[0].signature=wrong[1].signatures[0].signature;expect(()=>mergeApprovals(p,[],wrong)).toThrow();});
});}
