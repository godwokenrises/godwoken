// Emit a browser scenario using two transactions already verified by the real CKB-VM.
// Every API request is intercepted. No mainnet transaction or production key is used.
import { readFileSync } from "node:fs";
import { utils } from "@ckb-lumos/base";
const fixture = JSON.parse(
  readFileSync(
    new URL("./partial-claim-fixture.json", import.meta.url),
    "utf8",
  ),
);
function inventory(cells) {
  const balances = new Map();
  for (const c of cells) {
    balances.set(1, (balances.get(1) ?? 0n) + BigInt(c.output.capacity));
    if (!c.output.type) continue;
    const t = c.output.type;
    const h = utils.computeScriptHash({
      codeHash: t.code_hash,
      hashType: t.hash_type,
      args: t.args,
    });
    const id = fixture.config.assets.find((a) => a.type_hash === h).asset_id;
    const amount = Buffer.from(c.output_data.slice(2), "hex").reduceRight(
      (n, v) => n * 256n + BigInt(v),
      0n,
    );
    balances.set(id, (balances.get(id) ?? 0n) + amount);
  }
  return {
    address: fixture.request.address,
    omnilock_address: "",
    entitlements: [],
    available: [...balances].map(([asset_id, n]) => ({
      asset_id,
      raw: n.toString(),
    })),
    cells,
    explanations: [],
    more_batches: false,
    unsupported_cells: 0,
    confirming_cells: 0,
    confirmations_required: 24,
    observed_block: 128,
  };
}
fixture.inventories = [
  inventory(fixture.quote.input_cells),
  inventory(fixture.remaining_cells),
  inventory([]),
];
const scenario = async (page) => {
  const f = FIXTURE;
  let stage = 0,
    prepares = 0;
  await page.route("**/api/**", async (route) => {
    const pathname = new URL(route.request().url()).pathname;
    const match = pathname.match(/^\/api\/(v[01])\/(.+)$/);
    if (!match) throw Error("Unscoped API request");
    const protocol = match[1];
    const path = `/api/${match[2]}`;
    const scope = { protocol, plan_manifest: protocol === "v1" ? f.config.plan_manifest : "0x" + "ab".repeat(32) };
    if (route.request().method() === "POST") {
      const raw = route.request().postDataJSON();
      const scoped = path === "/api/submit" ? raw.request : raw;
      if (scoped.protocol !== protocol || scoped.plan_manifest !== scope.plan_manifest)
        throw Error("Wrong protocol/manifest request");
    }
    let body;
    if (path === "/api/config") body = f.config;
    else if (path.startsWith("/api/account/")) body = f.inventories[stage];
    else if (path === "/api/prepare") {
      prepares++;
      const request = route.request().postDataJSON().request;
      if (
        stage === 0 &&
        JSON.stringify(request.amounts) !== JSON.stringify(f.request.amounts)
      )
        throw Error("Wrong partial amounts");
      if (stage === 1 && request.amounts !== undefined)
        throw Error("Second transfer should take the remainder");
      const expected = stage === 0 ? f.request : f.second_request;
      if (JSON.stringify(request.inputs) !== JSON.stringify(expected.inputs))
        throw Error("Did not use fresh live inputs");
      body = stage === 0 ? f.quote : f.second_quote;
    } else if (path === "/api/submit") {
      const request = route.request().postDataJSON();
      const quote = stage === 0 ? f.quote : f.second_quote;
      if (
        request.signature !==
          (stage === 0 ? f.signature : f.second_signature) ||
        request.signing_message !== quote.signing_message
      )
        throw Error("Wrong signed transaction");
      stage++;
      body = { status: "submitted", transaction_hash: quote.transaction_hash };
    } else if (path.startsWith("/api/transaction/"))
      body = { status: "committed" };
    else throw Error("Unexpected request");
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ...body, ...scope }),
    });
  });
  await page.addInitScript((f) => {
    window.__partialSignCount = 0;
    window.ethereum = {
      isMetaMask: true,
      request: async ({ method, params }) => {
        if (method === "eth_requestAccounts" || method === "eth_accounts")
          return [f.request.address];
        if (method === "personal_sign") {
          window.__partialSignCount++;
          if (params[0] === f.quote.signing_message) return f.signature;
          if (params[0] === f.second_quote.signing_message)
            return f.second_signature;
        }
        throw Error("Unexpected wallet request");
      },
    };
    localStorage.clear();
  }, f);
  await page.goto("http://127.0.0.1:8787");
  await page.getByRole("button", { name: "Connect wallet ↗" }).click();
  await page
    .getByRole("button", { name: "MetaMask Connect →", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "CKB recipient address" })
    .fill(f.request.destination);
  await page.getByRole("checkbox").check();
  await page.locator("#amount-1").fill("600");
  await page.locator(`#amount-${f.request.amounts[1].asset_id}`).fill("0.5");
  await page.getByRole("button", { name: "Review transfer →" }).click();
  await page.getByRole("button", { name: "Sign & transfer on CKB ↗" }).click();
  await page.getByText("Transfer confirmed", { exact: true }).waitFor();
  await page.waitForFunction(() =>
    document.body.textContent.includes("0.73456789"),
  );
  await page
    .locator(`#amount-${f.request.amounts[1].asset_id}`)
    .fill("0.73456790");
  await page.getByRole("button", { name: "Review transfer →" }).click();
  await page
    .getByRole("alert")
    .filter({ hasText: "exceeds remaining balance" })
    .waitFor();
  if (
    stage !== 1 ||
    prepares !== 1 ||
    (await page.evaluate(() => window.__partialSignCount)) !== 1
  )
    throw Error("Overdraw reached signing or API prepare");
  await page.setViewportSize({ width: 390, height: 844 });
  if (
    await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  )
    throw Error("Mobile horizontal overflow");
  await page.screenshot({
    path: "exit-withdraw/claim-web/output/playwright/partial-overdraw.png",
    fullPage: true,
  });
  await page.getByRole("checkbox").uncheck();
  await page.getByRole("button", { name: "Review transfer →" }).click();
  await page.getByRole("button", { name: "Sign & transfer on CKB ↗" }).click();
  await page.waitForFunction(() => window.__partialSignCount === 2);
  await page
    .getByRole("heading", { name: "No available refund found" })
    .waitFor();
  if (stage !== 2 || prepares !== 2)
    throw Error("Remainder was not transferred exactly once");
  console.log(
    "PASS: partial withdrawal, confirmed remainder, excess-by-one rejected before API/signing, second withdrawal of all remainder, no mainnet submission.",
  );
};
console.log(scenario.toString().replace("FIXTURE", JSON.stringify(fixture)));
