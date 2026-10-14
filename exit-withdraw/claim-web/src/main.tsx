import { ControlPanel } from "./ControlPanel";
import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { formatUnits } from "ethers";
import type { BuildRequest, Inventory, Wallet } from "./types";
import { discover, current } from "./wallet";
import {
  destination,
  verifyQuote,
  verifyWalletSignature,
  transferTokens,
} from "./verify";
import { accountAssets } from "./assets";
import { withdrawalAmounts } from "./amounts";
import "./style.css";
import { endpoint, historyKey, verifyScope, type Protocol, type ScopedConfig, type ScopedQuote } from "./scope";
const short = (s: string) => `${s.slice(0, 8)}…${s.slice(-6)}`;
const display = (raw: string, decimals: number | null) =>
  decimals === null ? `${raw} raw units` : formatUnits(BigInt(raw), decimals);
async function api<T>(path: string, body?: unknown): Promise<T> {
  const r = await fetch(path, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await r.json();
  if (!r.ok)
    throw new Error(
      data.error ?? "The service is unavailable. Please try again.",
    );
  return data;
}
function App() {
  const [mode,setMode]=useState<"personal"|"contract">("personal");
  const [protocol, setProtocol] = useState<Protocol>("v1");
  const [config, setConfig] = useState<ScopedConfig>();
  const [wallets, setWallets] = useState<Wallet[]>([]);
  const [wallet, setWallet] = useState<Wallet>();
  const [address, setAddress] = useState("");
  const [inventory, setInventory] = useState<Inventory>();
  const [target, setTarget] = useState("");
  const [partial, setPartial] = useState(false);
  const [amounts, setAmounts] = useState<Record<number, string>>({});
  const [quote, setQuote] = useState<ScopedQuote>();
  const [request, setRequest] = useState<BuildRequest>();
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [tx, setTx] = useState("");
  const [txStatus, setTxStatus] = useState("");
  const [chooser, setChooser] = useState(false);
  const generation = useRef(0);
  const submitting = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const stop = discover((w) =>
      setWallets((old) =>
        old.some((x) => x.provider === w.provider) ? old : [...old, w],
      ),
    );
    return () => {
      alive.current = false;
      stop();
    };
  }, []);
  useEffect(() => {
    let cancelled = false;
    api<ScopedConfig>(endpoint(protocol, "config")).then(c => {
      if (cancelled) return;
      if (c.protocol !== protocol) throw new Error("Wrong protocol response");
      setConfig(c);
    }).catch(e => { if (!cancelled) setError(e.message); });
    return () => { cancelled = true; };
  }, [protocol]);
  useEffect(() => { if (address && config) void load(address); }, [protocol, config, address]);
  function chooseProtocol(next: Protocol) {
    clear();
    setConfig(undefined);
    setProtocol(next);
  }
  function clear() {
    generation.current++;
    setInventory(undefined);
    setQuote(undefined);
    setRequest(undefined);
    setTx("");
    setTxStatus("");
    setError("");
    setBusy("");
    setTarget("");
    setPartial(false);
    setAmounts({});
  }
  async function load(a: string, g = generation.current) {
    try {
      const result = await api<Inventory & {protocol: Protocol; plan_manifest: string}>(endpoint(protocol, `account/${a}`));
      if (config) verifyScope(result, protocol, config.plan_manifest);
      if (g === generation.current && alive.current) setInventory(result);
    } catch (e) {
      if (g === generation.current) setError((e as Error).message);
    }
  }
  useEffect(() => {
    if (!wallet) return;
    const changed = (accounts: unknown) => {
      clear();
      if (
        Array.isArray(accounts) &&
        typeof accounts[0] === "string" &&
        /^0x[\da-f]{40}$/i.test(accounts[0])
      ) {
        const a = accounts[0].toLowerCase();
        setAddress(a);
        void load(a);
      } else {
        setAddress("");
        setWallet(undefined);
      }
    };
    const chain = () => {
      generation.current++;
      setQuote(undefined);
      setRequest(undefined);
      setBusy("");
      if (address) void load(address);
    };
    const disconnect = () => {
      clear();
      setAddress("");
      setWallet(undefined);
    };
    wallet.provider.on?.("accountsChanged", changed);
    wallet.provider.on?.("chainChanged", chain);
    wallet.provider.on?.("disconnect", disconnect);
    return () => {
      wallet.provider.removeListener?.("accountsChanged", changed);
      wallet.provider.removeListener?.("chainChanged", chain);
      wallet.provider.removeListener?.("disconnect", disconnect);
    };
  }, [wallet, address, protocol, config]);
  useEffect(() => {
    if (!address || !config) return;
    try {
      const h = localStorage.getItem(historyKey(protocol, config?.plan_manifest ?? "", address));
      if (h && /^0x[\da-f]{64}$/.test(h)) {
        setTx(h);
        setTxStatus("checking");
      }
    } catch {
      /* Storage can be unavailable in private mode. */
    }
  }, [address, protocol, config?.plan_manifest]);
  useEffect(() => {
    if (!tx || !address) return;
    let cancelled = false;
    let timer: number;
    let attempts = 0;
    const a = address;
    async function poll() {
      try {
        const r = await api<{ status: string | null }>(
          endpoint(protocol, `transaction/${tx}`),
        );
        if (cancelled) return;
        const status = r.status ?? "not found";
        setTxStatus(status);
        if (status === "committed") {
          void load(a);
          return;
        }
        if (status === "rejected") return;
      } catch {
        if (!cancelled) setTxStatus("status unavailable");
      }
      if (!cancelled && ++attempts < 180) timer = window.setTimeout(poll, 5000);
    }
    void poll();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [tx, address, protocol, config]);
  async function connect(w: Wallet) {
    setError("");
    setBusy("Connecting wallet…");
    const g = generation.current;
    try {
      await w.provider.request({ method: "eth_requestAccounts" });
      const a = await current(w.provider);
      if (g !== generation.current) return;
      clear();
      setWallet(w);
      setAddress(a);
      setChooser(false);
      setBusy("Loading your refund…");
      await load(a);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy("");
    }
  }
  async function review() {
    if (!address || !inventory || !config || !wallet) return;
    setError("");
    setQuote(undefined);
    setBusy("Checking cells and preparing your transfer…");
    const g = generation.current;
    try {
      destination(target);
      if ((await current(wallet.provider)) !== address)
        throw new Error(
          "Your wallet account changed. Reconnect before continuing.",
        );
      const r = {
        address,
        destination: target,
        inputs: inventory.cells.map((c) => c.out_point),
        ...(partial
          ? {
              amounts: withdrawalAmounts(
                amounts,
                inventory.available,
                config.assets,
              ),
            }
          : {}),
      };
      const q = await api<ScopedQuote>(endpoint(protocol, "prepare"), {protocol, plan_manifest: config.plan_manifest, request: r});
      verifyScope(q, protocol, config.plan_manifest);
      verifyQuote(q, r, config);
      if (g === generation.current) {
        setRequest(r);
        setQuote(q);
      }
    } catch (e) {
      if (g === generation.current) setError((e as Error).message);
    } finally {
      if (g === generation.current) setBusy("");
    }
  }
  async function transfer() {
    if (!wallet || !quote || !request || !config || submitting.current) return;
    const g = generation.current;
    const q = quote;
    const r = request;
    const w = wallet;
    let attemptedSend = false;
    submitting.current = true;
    setError("");
    setBusy("Confirm the signature in your wallet…");
    try {
      verifyScope(q, protocol, config.plan_manifest);
      verifyQuote(q, r, config);
      if ((await current(w.provider)) !== r.address || g !== generation.current)
        throw new Error("Wallet account changed. Review a new transfer.");
      const sig = await w.provider.request({
        method: "personal_sign",
        params: [q.signing_message, r.address],
      });
      if ((await current(w.provider)) !== r.address || g !== generation.current)
        throw new Error("Wallet changed while signing. Nothing was submitted.");
      if (typeof sig !== "string")
        throw new Error("Wallet returned an invalid signature.");
      verifyWalletSignature(q.signing_message, sig, r.address);
      setBusy("Validating and submitting on CKB…");
      // Preserve the reviewed hash before sending: a lost HTTP response must not trigger a blind retry.
      try {
        localStorage.setItem(historyKey(protocol, config.plan_manifest, r.address), q.transaction_hash);
      } catch {}
      attemptedSend = true;
      const result = await api<{ transaction_hash: string }>(endpoint(protocol, "submit"), {
        request: { protocol, plan_manifest: config.plan_manifest, request: r },
        transaction_hash: q.transaction_hash,
        signing_message: q.signing_message,
        signature: sig,
      });
      if (g === generation.current) {
        setTx(result.transaction_hash);
        setTxStatus("submitted");
        setQuote(undefined);
        setRequest(undefined);
      }
    } catch (e) {
      if (g === generation.current) {
        setError((e as Error).message);
        if (attemptedSend) {
          setTx(q.transaction_hash);
          setTxStatus("checking");
          setQuote(undefined);
          setRequest(undefined);
        }
      }
    } finally {
      submitting.current = false;
      if (g === generation.current) setBusy("");
    }
  }
  async function refresh() {
    if (!address) return;
    setError("");
    setQuote(undefined);
    setRequest(undefined);
    setBusy("Refreshing live CKB cells…");
    await load(address);
    setBusy("");
  }
  const ready = inventory?.cells.length ?? 0;
  const assets = inventory
    ? accountAssets(config?.assets ?? [], inventory)
    : [];
  return (
    <div className="shell">
      <header>
        <a className="brand" href="/" aria-label="Godwoken refunds home">
          <span className="brand-mark">g</span> godwoken{" "}
          <span className="brand-divider">/</span>
          <span className="brand-section">Withdraw</span>
        </a>
        <div className="header-right">
          <span className="network">
            <i /> CKB Mainnet
          </span>
          {address ? (
            <button
              className="wallet-button"
              onClick={() => {
                clear();
                setAddress("");
                setWallet(undefined);
              }}
              title="Disconnect wallet"
            >
              {short(address)} <span>↗</span>
            </button>
          ) : (
            <button
              className="wallet-button"
              disabled={!config || !!busy}
              onClick={() => setChooser(!chooser)}
            >
              Connect wallet <span>↗</span>
            </button>
          )}
        </div>
      </header>
      <main>
        <section className="intro">
          <div className="eyebrow">GODWOKEN {protocol.toUpperCase()} · REFUND & WITHDRAW</div>
          <h1>
            Your assets.
            <br />
            <span>Back in your hands.</span>
          </h1>
          <p>
            View your refund and move available assets to your CKB wallet.
            <br className="desktop" /> Use the Ethereum wallet you used on
            Godwoken.
          </p>
          <div className="scope-note">
            <label htmlFor="protocol">Refund protocol </label>
            <select id="protocol" disabled={!!busy} value={protocol} onChange={e => chooseProtocol(e.target.value as Protocol)}>
              <option value="v1">Godwoken v1 (default)</option>
              <option value="v0">Godwoken v0</option>
            </select>
            <p>Only {protocol} refund assets are shown and transferred. Switching clears the current transfer review.</p>
          </div>
        </section>
        {chooser && (
          <section
            className="wallet-picker"
            aria-label="Choose an Ethereum wallet"
          >
            <div className="section-heading">
              <h2>Connect an Ethereum wallet</h2>
              <button className="text-button" onClick={() => setChooser(false)}>
                Close
              </button>
            </div>
            {wallets.length ? (
              wallets.map((w) => (
                <button
                  key={w.id}
                  onClick={() => void connect(w)}
                  disabled={!!busy}
                >
                  {w.name}
                  <span>Connect →</span>
                </button>
              ))
            ) : (
              <p>
                No Ethereum wallet was found. Open this page in your wallet
                browser or install MetaMask, Rabby, or another compatible
                extension, then reload.
              </p>
            )}
            <small>
              Connecting lets us read your public address. No signature or ETH
              gas is needed.
            </small>
          </section>
        )}
        {error && (
          <div className="alert" role="alert">
            <strong>We couldn’t complete that step.</strong>
            <p>{error}</p>
            {quote && (
              <a
                href={`https://explorer.nervos.org/transaction/${quote.transaction_hash}`}
                target="_blank"
                rel="noreferrer"
              >
                Check the reviewed transaction on CKB ↗
              </a>
            )}
          </div>
        )}
        {busy && (
          <div className="progress" role="status">
            <span className="spinner" />
            {busy}
          </div>
        )}
        <nav aria-label="Refund account mode">
          <button disabled={!!busy} aria-pressed={mode==="personal"} onClick={()=>{clear();setMode("personal")}}>Personal Account</button>
          <button disabled={!!busy} aria-pressed={mode==="contract"} onClick={()=>{clear();setMode("contract")}}>Contract Account</button>
        </nav>
        {mode==="contract" && config && <ControlPanel key={`${protocol}:${config.plan_manifest}:${address}`} config={config} address={address} wallet={wallet} onBusy={setBusy}/>}
        <div className="workspace" hidden={mode!=="personal"} style={mode!=="personal"?{display:"none"}:undefined}>
          <section className="card assets-card">
            <div className="card-heading">
              <div>
                <span className="step">01 / YOUR REFUND</span>
                <h2>Refund assets</h2>
              </div>
              {address && (
                <button
                  className="text-button"
                  disabled={!!busy}
                  onClick={() => void refresh()}
                >
                  ↻ Refresh
                </button>
              )}
            </div>
            {!address ? (
              <div className="empty">
                <div className="empty-icon">↙</div>
                <h3>Start with your ETH wallet</h3>
                <p>
                  Connect to find the refund associated
                  <br />
                  with your Ethereum address.
                </p>
                <button
                  className="primary"
                  onClick={() => setChooser(true)}
                  disabled={!config}
                >
                  Connect wallet →
                </button>
              </div>
            ) : !inventory ? (
              <div className="empty">
                <span className="empty-icon">◌</span>
                <h3>Looking up your assets</h3>
                <p>Checking your refund record and live CKB cells.</p>
              </div>
            ) : (
              <>
                <div className="account-strip">
                  <span>Connected ETH address</span>
                  <code title={address}>{address}</code>
                </div>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>Asset</th>
                        <th>Approved entitlement</th>
                        <th>Available on CKB</th>
                      </tr>
                    </thead>
                    <tbody>
                      {assets.map((a) => {
                        const { entitlement, available } = a;
                        return (
                          <React.Fragment key={a.asset_id}>
                            <tr data-asset-id={a.asset_id}>
                              <td>
                                <span
                                  className={`asset-icon ${a.symbol === "CKB" ? "ckb" : ""}`}
                                >
                                  {(a.symbol ?? "?").slice(0, 1)}
                                </span>
                                <strong>
                                  {a.symbol ?? `Token #${a.asset_id}`}
                                </strong>
                                <small>Asset #{a.asset_id}</small>
                              </td>
                              <td title={display(entitlement, a.decimals)}>
                                {display(entitlement, a.decimals)}
                              </td>
                              <td
                                className={
                                  available === "0" ? "muted" : "amount"
                                }
                                title={display(available, a.l1_decimals)}
                              >
                                {display(available, a.l1_decimals)}
                              </td>
                            </tr>
                            {!!a.explanations.length && (
                              <tr className="asset-disposition">
                                <td colSpan={3}>
                                  {a.explanations.map((row, i) => (
                                    <div key={i}>
                                      <strong>
                                        {row.status.replaceAll("_", " ")}
                                      </strong>
                                      <p>
                                        Calculated snapshot rights:{" "}
                                        {row.calculated_entitlement}{" "}
                                        {a.symbol ?? `Token #${a.asset_id}`}
                                      </p>
                                      <p>{row.reason}</p>
                                    </div>
                                  ))}
                                </td>
                              </tr>
                            )}
                          </React.Fragment>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
                {!assets.length && !ready && (
                  <div className="empty compact">
                    <h3>No available refund found</h3>
                    <p>
                      This address has no payable allocation in the published
                      plan and no supported live cells.
                    </p>
                  </div>
                )}
                {!ready && !!assets.length && (
                  <div className="inline-note">
                    No confirmed assets are available to transfer. Your refund
                    may be awaiting distribution, held under the refund rules,
                    or already transferred. See each asset’s disposition in the
                    table.
                  </div>
                )}
                <details>
                  <summary>Your CKB holding address</summary>
                  <p className="holding-address">
                    {inventory.omnilock_address}
                  </p>
                  <p>
                    These assets are controlled by your connected Ethereum key.
                  </p>
                </details>
                <div className="table-note">
                  Approved entitlement is the published snapshot allocation, not
                  an unpaid balance. Available assets are confirmed cells traced to this
                  protocol’s published refund release, including unmixed partial-withdrawal change. CKB includes
                  token-cell capacity. Each asset associated with this address
                  is listed, including held and excluded snapshot rights. These
                  rights do not add to the approved or available amounts.
                </div>
                {inventory.more_batches && (
                  <div className="inline-note">
                    Showing the next 64 cells. Transfer this batch, then refresh
                    to continue.
                  </div>
                )}
                {inventory.confirming_cells > 0 && (
                  <div className="inline-note">
                    {inventory.confirming_cells} cells are awaiting{" "}
                    {inventory.confirmations_required} CKB confirmations.
                  </div>
                )}
                {(inventory.unattributed_cells ?? 0) > 0 && (
                  <div className="inline-note">{inventory.unattributed_cells} cells do not have an unmixed origin in the selected refund release and are excluded from this transfer.</div>
                )}
                {inventory.unsupported_cells > 0 && (
                  <div className="inline-note">
                    {inventory.unsupported_cells} unsupported cells are excluded
                    and will remain in your wallet.
                  </div>
                )}
                {!!inventory.explanations.length && (
                  <details>
                    <summary>How your refund was calculated</summary>
                    <p>
                      Amounts below describe the frozen snapshot. Held or
                      excluded rights are not transferable.
                    </p>
                    {inventory.explanations.map((row, i) => (
                      <div className="explanation" key={i}>
                        <strong>
                          {row.symbol ?? `Token #${row.asset_id}`} ·{" "}
                          {row.calculated_entitlement}
                        </strong>
                        <p>{row.protocols.join(" · ")}</p>
                        <p>{row.reason}</p>
                      </div>
                    ))}
                  </details>
                )}
              </>
            )}
          </section>
          <aside className="card transfer-card">
            <div className="card-heading">
              <div>
                <span className="step">02 / TRANSFER ON CKB</span>
                <h2>Choose your destination</h2>
              </div>
              <span className="arrow-icon">↗</span>
            </div>
            <p className="transfer-description">
              Send available assets to a CKB mainnet address you control.
            </p>
            <label htmlFor="destination">CKB recipient address</label>
            <textarea
              id="destination"
              placeholder="ckb1…"
              spellCheck={false}
              autoComplete="off"
              value={target}
              disabled={
                !ready ||
                !!busy ||
                txStatus === "pending" ||
                txStatus === "submitted"
              }
              onChange={(e) => {
                setTarget(e.target.value);
                setQuote(undefined);
                setRequest(undefined);
                setError("");
              }}
            />
            <div className="field-help">
              A CKB Layer 1 transfer. No Ethereum bridge or ETH gas is involved.
            </div>
            <fieldset
              className="withdraw-amounts"
              disabled={
                !ready ||
                !!busy ||
                ["pending", "submitted", "checking"].includes(txStatus)
              }
            >
              <legend>Amount to withdraw</legend>
              <label>
                <input
                  type="checkbox"
                  checked={partial}
                  onChange={(e) => {
                    setPartial(e.target.checked);
                    setQuote(undefined);
                    setRequest(undefined);
                    setError("");
                  }}
                />{" "}
                Choose amounts (leave the rest in your wallet)
              </label>
              {partial && inventory && config && (
                <>
                  <p className="field-help">
                    Enter CKB including capacity for the tokens you receive. The
                    network fee is additional. Remaining assets stay under your
                    ETH wallet’s OmniLock and can be withdrawn after
                    confirmation. Both recipient and change cells need
                    sufficient CKB.
                  </p>
                  {inventory.available.map((b) => {
                    const a = config.assets.find(
                      (a) => a.asset_id === b.asset_id,
                    );
                    const name = a?.symbol ?? `Token #${b.asset_id}`;
                    return (
                      <label key={b.asset_id} htmlFor={`amount-${b.asset_id}`}>
                        {name} to withdraw
                        {a?.l1_decimals == null ? " (raw units)" : ""}
                        <input
                          id={`amount-${b.asset_id}`}
                          type="text"
                          inputMode="decimal"
                          autoComplete="off"
                          placeholder="0"
                          value={amounts[b.asset_id] ?? ""}
                          onChange={(e) => {
                            setAmounts((old) => ({
                              ...old,
                              [b.asset_id]: e.target.value,
                            }));
                            setQuote(undefined);
                            setRequest(undefined);
                            setError("");
                          }}
                        />
                        <span className="field-help">
                          Available: {display(b.raw, a?.l1_decimals ?? null)}
                        </span>
                      </label>
                    );
                  })}
                </>
              )}
            </fieldset>
            <div className="transfer-facts">
              <div>
                <span>From</span>
                <strong>{address ? short(address) : "Your ETH wallet"}</strong>
              </div>
              <div>
                <span>Network</span>
                <strong>CKB Mainnet</strong>
              </div>
              <div>
                <span>Assets</span>
                <strong>
                  {ready
                    ? `${ready} live ${ready === 1 ? "cell" : "cells"}`
                    : address
                      ? "No available cells"
                      : "Waiting for wallet"}
                </strong>
              </div>
              <div>
                <span>Network fee</span>
                <strong>
                  {quote
                    ? `${display(quote.fee_shannons, 8)} CKB`
                    : "Calculated before signing"}
                </strong>
              </div>
            </div>
            {!quote ? (
              <button
                className="primary full"
                disabled={
                  !ready ||
                  !target ||
                  !!busy ||
                  ["pending", "submitted", "checking"].includes(txStatus)
                }
                onClick={() => void review()}
              >
                Review transfer <span>→</span>
              </button>
            ) : (
              <div className="review">
                <h3>Review before signing</h3>
                <p className="recipient">{quote.destination}</p>
                <div className="review-value">
                  <span>CKB arriving at destination</span>
                  <strong>
                    {display(quote.received_capacity_shannons, 8)} CKB
                  </strong>
                </div>
                {config &&
                  transferTokens(quote, config).map(({ asset, raw }) => (
                    <div className="review-value" key={asset.asset_id}>
                      <span>
                        {asset.symbol ?? `Token #${asset.asset_id}`} arriving at
                        destination
                      </span>
                      <strong>{display(raw, asset.l1_decimals)}</strong>
                    </div>
                  ))}
                <p>
                  {request?.amounts
                    ? "Only the amounts shown above will be sent. The rest returns to your own OmniLock for a later withdrawal."
                    : "All token amounts shown as available will be transferred unchanged."}{" "}
                  The CKB amount includes capacity held in token cells.
                </p>
                <p>
                  Your wallet will ask you to sign a transaction hash. This
                  authorizes the CKB transfer reviewed here.
                </p>
                <button
                  className="primary full"
                  disabled={!!busy || !config?.broadcast_enabled}
                  onClick={() => void transfer()}
                >
                  {config?.broadcast_enabled
                    ? "Sign & transfer on CKB"
                    : "Transfers not enabled yet"}{" "}
                  <span>↗</span>
                </button>
                <button
                  className="text-button full"
                  disabled={!!busy}
                  onClick={() => {
                    setQuote(undefined);
                    setRequest(undefined);
                  }}
                >
                  Back to edit
                </button>
              </div>
            )}
            {!config?.broadcast_enabled && config && (
              <p className="operator-note">
                The portal is currently open for lookup and review. The operator
                has not enabled transaction submission.
              </p>
            )}
            {tx && (
              <div className="receipt" role="status">
                <strong>
                  {txStatus === "committed"
                    ? "Transfer confirmed"
                    : txStatus === "rejected"
                      ? "Transaction rejected"
                      : "Transfer status"}
                </strong>
                <p>
                  {txStatus === "committed"
                    ? "Your assets have arrived at the destination lock."
                    : `CKB status: ${txStatus}. Check the transaction before retrying.`}
                </p>
                <a
                  href={`https://explorer.nervos.org/transaction/${tx}`}
                  target="_blank"
                  rel="noreferrer"
                >
                  View transaction {short(tx)} ↗
                </a>
                <button
                  className="text-button"
                  onClick={() => {
                    try {
                      localStorage.removeItem(historyKey(protocol, config?.plan_manifest ?? "", address));
                    } catch {}
                    setTx("");
                    setTxStatus("");
                    void refresh();
                  }}
                >
                  Refresh balance
                </button>
              </div>
            )}
          </aside>
        </div>
        <section className="how">
          <div>
            <span>01</span>
            <h3>Connect your wallet</h3>
            <p>
              Use MetaMask or another Ethereum wallet with your original
              Godwoken address.
            </p>
          </div>
          <div>
            <span>02</span>
            <h3>Review your assets</h3>
            <p>
              See your approved refund and what is ready to transfer on CKB.
            </p>
          </div>
          <div>
            <span>03</span>
            <h3>Sign and receive</h3>
            <p>
              Authorize the L1 transfer. Assets go to the CKB address you
              choose.
            </p>
          </div>
        </section>
      </main>
      <footer>
        <span>Godwoken · Mainnet {protocol} refunds</span>
        <span>Your keys stay in your wallet.</span>
        <a href="https://explorer.nervos.org" target="_blank" rel="noreferrer">
          CKB Explorer ↗
        </a>
      </footer>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
