// Emit a Playwright CLI scenario. Every API request is intercepted; only synthetic keys are used.
import { readFileSync } from "node:fs";
import { utils } from "@ckb-lumos/base";
const fixture = JSON.parse(
  readFileSync(new URL("./claim-fixture.json", import.meta.url), "utf8"),
);
const available = new Map([[1, 140000000000n]]);
for (const cell of fixture.quote.input_cells) {
  if (!cell.output.type) continue;
  const s = cell.output.type;
  const h = utils.computeScriptHash({
    codeHash: s.code_hash,
    hashType: s.hash_type,
    args: s.args,
  });
  const a = fixture.config.assets.find((a) => a.type_hash === h);
  const b = Buffer.from(cell.output_data.slice(2), "hex");
  const n = b.reduceRight((n, v) => n * 256n + BigInt(v), 0n);
  available.set(a.asset_id, (available.get(a.asset_id) ?? 0n) + n);
}
fixture.inventory = {
  address: fixture.request.address,
  omnilock_address: "",
  entitlements: [{ asset_id: 1, raw: "1400000000000000000000" }],
  available: [...available].map(([asset_id, raw]) => ({
    asset_id,
    raw: raw.toString(),
  })),
  cells: fixture.quote.input_cells,
  explanations: [
    {
      asset_id: 99998,
      symbol: "HELD",
      calculated_entitlement: "0.123456789012345678",
      status: "DEFERRED",
      reason: "Awaiting review",
      protocols: [],
    },
    {
      asset_id: 99999,
      symbol: "EXCLUDED",
      calculated_entitlement: "42.0",
      status: "EXCLUDED_REWARD",
      reason: "Rewards are not refundable",
      protocols: [],
    },
  ],
  more_batches: false,
  unsupported_cells: 0,
  confirming_cells: 0,
  confirmations_required: 24,
  observed_block: 128,
};
const scenario = async (page) => {
  const f = FIXTURE;
  let submitted = false;
  let submitCalls = 0;
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
    else if (path.startsWith("/api/account/"))
      body = path.endsWith(f.request.address)
        ? {
            ...f.inventory,
            cells: submitted ? [] : f.inventory.cells,
            available: submitted ? [] : f.inventory.available,
          }
        : {
            ...f.inventory,
            address: path.split("/").pop(),
            cells: [],
            available: [],
            entitlements: [],
            explanations: [],
          };
    else if (path === "/api/prepare") body = f.quote;
    else if (path === "/api/submit") {
      submitCalls++;
      const b = route.request().postDataJSON();
      if (
        b.signature !== f.signature ||
        b.signing_message !== f.quote.signing_message
      )
        throw Error("Wrong submitted signature");
      submitted = true;
      body = {
        transaction_hash: f.quote.transaction_hash,
        status: "submitted",
      };
    } else if (path.startsWith("/api/transaction/"))
      body = { status: submitted ? "committed" : "unknown" };
    else throw Error("Unexpected API route");
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ...body, ...scope }),
    });
  });
  await page.addInitScript((f) => {
    const listeners = {};
    window.__claimTest = {
      listeners,
      address: f.request.address,
      reject: false,
      signRequests: [],
    };
    const provider = {
      isMetaMask: true,
      on: (e, cb) => {
        (listeners[e] ??= []).push(cb);
      },
      removeListener: (e, cb) => {
        listeners[e] = (listeners[e] ?? []).filter((x) => x !== cb);
      },
      request: async ({ method, params }) => {
        if (method === "eth_requestAccounts" || method === "eth_accounts")
          return [window.__claimTest.address];
        if (method === "personal_sign") {
          if (window.__claimTest.reject)
            throw Error("User rejected the signature");
          if (
            params[0] !== f.quote.signing_message ||
            params[1] !== f.request.address
          )
            throw Error("Wrong personal_sign parameters");
          window.__claimTest.signRequests.push(params);
          return f.signature;
        }
        throw Error("Unexpected wallet method");
      },
    };
    window.ethereum = provider;
    window.addEventListener("eip6963:requestProvider", () =>
      window.dispatchEvent(
        new CustomEvent("eip6963:announceProvider", {
          detail: {
            info: { uuid: "synthetic-wallet", name: "Synthetic MetaMask" },
            provider,
          },
        }),
      ),
    );
    localStorage.clear();
  }, f);
  await page.goto("http://127.0.0.1:8787");
  await page.getByRole("button", { name: "Connect wallet ↗" }).click();
  await page
    .getByRole("button", { name: "Synthetic MetaMask Connect →" })
    .click();
  await page.locator('[data-asset-id="99998"]').waitFor();
  if (await page.locator("#protocol").inputValue() !== "v1") throw Error("Default is not v1");
  await page.locator("#protocol").selectOption("v0");
  await page.getByText("Only v0 refund assets", { exact: false }).waitFor();
  await page.locator('[data-asset-id="99998"]').waitFor();
  await page.locator("#protocol").selectOption("v1");
  await page.getByText("Only v1 refund assets", { exact: false }).waitFor();
  await page.locator('[data-asset-id="99998"]').waitFor();
  for (const id of [1, 36, 161, 99998, 99999]) {
    if ((await page.locator(`[data-asset-id="${id}"]`).count()) !== 1)
      throw Error(`Missing or duplicated asset ${id}`);
  }
  if (
    !(await page.locator(".asset-disposition").allTextContents())
      .join(" ")
      .includes("0.123456789012345678")
  )
    throw Error("Missing held snapshot amount");
  await page
    .getByRole("textbox", { name: "CKB recipient address" })
    .fill("ckt1bad");
  await page.getByRole("button", { name: "Review transfer →" }).click();
  if (!(await page.getByRole("alert").innerText()).includes("mainnet"))
    throw Error("Missing invalid-network error");
  const review = async () => {
    await page
      .getByRole("textbox", { name: "CKB recipient address" })
      .fill(f.request.destination);
    await page.getByRole("button", { name: "Review transfer →" }).click();
    await page
      .getByRole("button", { name: "Sign & transfer on CKB ↗" })
      .waitFor();
  };
  await review();
  // A wallet account change invalidates the previous quote and its sign control.
  await page.evaluate(() => {
    window.__claimTest.address = "0x" + "22".repeat(20);
    window.__claimTest.listeners.accountsChanged.forEach((cb) =>
      cb([window.__claimTest.address]),
    );
  });
  await page
    .getByRole("heading", { name: "No available refund found" })
    .waitFor();
  if (
    await page
      .getByRole("button", { name: "Sign & transfer on CKB ↗" })
      .count()
  )
    throw Error("Stale quote after account switch");
  await page.evaluate((a) => {
    window.__claimTest.address = a;
    window.__claimTest.listeners.accountsChanged.forEach((cb) => cb([a]));
  }, f.request.address);
  await review();
  await page.evaluate(() => {
    window.__claimTest.reject = true;
  });
  await page.getByRole("button", { name: "Sign & transfer on CKB ↗" }).click();
  await page
    .getByText("User rejected the signature", { exact: true })
    .waitFor();
  if (submitCalls !== 0) throw Error("Submitted after wallet rejection");
  await page.evaluate(() => {
    window.__claimTest.reject = false;
  });
  await page.setViewportSize({ width: 390, height: 844 });
  if (
    await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth,
    )
  )
    throw Error("Mobile horizontal overflow");
  await page.getByRole("button", { name: "Sign & transfer on CKB ↗" }).click();
  await page.getByText("Transfer confirmed", { exact: true }).waitFor();
  if (
    submitCalls !== 1 ||
    (await page.evaluate(() => window.__claimTest.signRequests.length)) !== 1
  )
    throw Error("Duplicate signing/submission");
  await page.screenshot({
    path: "exit-withdraw/claim-web/output/playwright/mobile.png",
    fullPage: true,
  });
  console.log(
    "PASS: EIP-6963 wallet; invalid destination; Rust digest; account switch; signature rejection; one submission; confirmation; mobile layout. No mainnet submission.",
  );
};
console.log(scenario.toString().replace("FIXTURE", JSON.stringify(fixture)));
