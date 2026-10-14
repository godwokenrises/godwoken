import {readFileSync} from "node:fs";
const fixtures=Object.fromEntries(["v0","v1"].map(v=>[v,JSON.parse(readFileSync(new URL(`./control-${v}-fixture.json`,import.meta.url),"utf8"))]));
const scenario=async(page)=>{
 const fixtures=FIXTURES;let submits=0;
 await page.route("**/api/**",async route=>{
  const [_,protocol,path]=new URL(route.request().url()).pathname.match(/^\/api\/(v[01])\/(.+)$/)??[];
  if(!protocol)throw Error("Unscoped request");const f=fixtures[protocol],p=f.response;let data;
  if(path==="config")data={protocol,plan_manifest:p.plan_manifest,assets:[{asset_id:1,symbol:"CKB",decimals:protocol==="v1"?18:8,l1_decimals:8,type_hash:"0x"+"00".repeat(32)}],dependencies:[],broadcast_enabled:true,fee_rate:1000};
  else if(path==="controls")data={protocol,plan_manifest:p.plan_manifest,accounts:[{account_id:4,threshold:2,owners:f.owners,entitlements:[],explanations:[{asset_id:1,symbol:"CKB",calculated_entitlement:"500",status:"APPROVED",reason:"Test principal"},{asset_id:3,symbol:"TEST",calculated_entitlement:"100",status:"APPROVED",reason:"Test token"}]}]};
  else if(path.startsWith("account/"))data={address:f.owner,entitlements:[],available:[],cells:[],explanations:[],more_batches:false};
  else if(path.startsWith("control-proposal/"))data=p;
  else if(path==="control-submit") {const b=route.request().postDataJSON();if(b.protocol!==protocol||b.plan_manifest!==p.plan_manifest||b.transaction_hash!==p.signing_request.transaction_hash)throw Error("Wrong submission context");if(b.signatures.length!==2)throw Error("Missing signatures");submits++;data={transaction_hash:b.transaction_hash,status:"submitted"};}
  else if(path.startsWith("transaction/"))data={status:"committed"};else throw Error(path);
  await route.fulfill({json:data});
 });
 await page.addInitScript(fixtures=>{
  localStorage.clear();window.__reject=false;
  const owner=fixtures.v1.owner;
  const provider={request:async({method,params})=>{
   if(method==="eth_accounts"||method==="eth_requestAccounts")return [owner];
   if(method==="personal_sign") {if(window.__reject)throw Error("User rejected signing");for(const f of Object.values(fixtures)){const g=f.response.signing_request.groups.find(g=>g.message===params[0] && g.identities.includes(owner));if(g){const sig=f.approvals.flatMap(a=>a.signatures).find(s=>s.lock_hash===g.lock_hash);return sig.signature;}}throw Error("Unexpected signing message");}
   throw Error(method);
  },on:()=>{},removeListener:()=>{}};
  window.ethereum=provider;
  const announce=()=>window.dispatchEvent(new CustomEvent("eip6963:announceProvider",{detail:{info:{uuid:"control-test",name:"Control Test Wallet"},provider}}));window.addEventListener("eip6963:requestProvider",announce);
 },fixtures);
 await page.goto("http://127.0.0.1:5173");await page.getByRole("button",{name:/Connect wallet/}).first().click();await page.getByRole("button",{name:/Control Test Wallet/}).first().click();
 await page.getByRole("button",{name:"Contract Account",exact:true}).click();
 for(const protocol of ["v1","v0"]){
  if(protocol==="v0")await page.getByLabel("Refund protocol").selectOption("v0");
  const f=fixtures[protocol],p=f.response;
  await page.getByRole("combobox",{name:"Contract account",exact:true}).selectOption("4");
  await page.getByLabel("Proposal transaction hash").fill(p.signing_request.transaction_hash);await page.getByRole("button",{name:"Load proposal"}).click();
  await page.getByRole("heading",{name:"Review every transaction output"}).waitFor();
  await page.setViewportSize({width:390,height:844});
  if(await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth+1))throw Error("Contract view mobile overflow");
  await page.screenshot({path:`exit-withdraw/claim-web/output/playwright/control-${protocol}-mobile.png`,fullPage:true});
  await page.setViewportSize({width:1280,height:900});
  await page.evaluate(()=>{window.__reject=true});await page.getByRole("button",{name:"Sign and download approval"}).click();await page.getByRole("alert").filter({hasText:"User rejected"}).waitFor();
  await page.evaluate(()=>{window.__reject=false});await page.getByRole("button",{name:"Sign and download approval"}).click();await page.getByText("1 distinct ETH approvals collected.",{exact:false}).waitFor();
  const own=p.signing_request.groups.find(g=>g.identities.includes(f.owner));const other=f.approvals.filter(a=>a.signatures.every(s=>s.lock_hash!==own.lock_hash));
  await page.getByLabel("Import co-signer approvals").setInputFiles({name:"approval.json",mimeType:"application/json",buffer:Buffer.from(JSON.stringify(other))});await page.getByText("2 distinct ETH approvals collected.",{exact:false}).waitFor();
  await page.getByRole("button",{name:"Submit jointly signed refund"}).click();await page.getByText(`Submitted: ${p.signing_request.transaction_hash}`,{exact:true}).waitFor();
  if(!await page.getByRole("button",{name:"Submit jointly signed refund"}).isDisabled())throw Error("Repeat submit remains enabled");
  await page.getByRole("button",{name:"Check transaction"}).click();await page.getByText(`Transaction ${p.signing_request.transaction_hash}: committed`,{exact:true}).waitFor();
 }
 if(submits!==2)throw Error(`Wrong submission count: ${submits}`);
 await page.getByRole("button",{name:"Personal Account",exact:true}).click();if(await page.getByLabel("Contract account refunds").count())throw Error("Contract signing context leaked into personal mode");
 await page.screenshot({path:"exit-withdraw/claim-web/output/playwright/control-browser.png",fullPage:true});
 console.log("PASS: V0/V1 control account tabs, Rust-derived OmniLock messages, wallet rejection, independent approval exchange, one submission per proposal, confirmation and tab isolation.");
};
console.log(scenario.toString().replace("FIXTURES",JSON.stringify(fixtures)));
