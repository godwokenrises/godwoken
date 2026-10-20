import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { formatUnits } from "ethers";
import { encodeToAddress } from "@ckb-lumos/helpers";
import { predefined } from "@ckb-lumos/config-manager";
import { discover, current } from "./wallet";
import {
  destination,
  OMNI,
  transferTokens,
  verifyQuote,
  verifyWalletSignature,
} from "./verify";
import { withdrawalAmounts } from "./amounts";
import type {
  BuildRequest,
  Config,
  Quote,
  Wallet,
  WalletHistory,
  WalletInventory,
} from "./types";
import "./style.css";

type WalletConfig = Omit<Config, "plan_manifest" | "snapshot_root">;
const short = (s: string) => `${s.slice(0, 10)}…${s.slice(-8)}`;
const display = (s: string, d: number | null) =>
  d == null ? `${s} raw units` : formatUnits(BigInt(s), d);
const explorerTx = (hash: string) =>
  `https://explorer.nervos.org/transaction/${hash}`;
const historyKey = (owner: string) => `omnilock-mainnet:pending:${owner}`;
const omni = (eth: string) =>
  encodeToAddress(
    { codeHash: OMNI, hashType: "type", args: `0x01${eth.slice(2)}00` },
    { config: predefined.LINA },
  );
async function api<T>(path: string, body?: unknown): Promise<T> {
  const r = await fetch(`/api/wallet/${path}`, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await r.json();
  if (!r.ok || data.error)
    throw Object.assign(
      Error(data.error ?? "Request failed. Refresh and try again."),
      {
        notSubmitted: data.broadcast_attempted === false,
      },
    );
  return data;
}
function App() {
  const [config, setConfig] = useState<WalletConfig>();
  const [wallets, setWallets] = useState<Wallet[]>([]);
  const [wallet, setWallet] = useState<Wallet>();
  const [address, setAddress] = useState("");
  const [inventory, setInventory] = useState<WalletInventory>();
  const [history, setHistory] = useState<WalletHistory>();
  const [historyError, setHistoryError] = useState("");
  const [target, setTarget] = useState("");
  const [partial, setPartial] = useState(false);
  const [amounts, setAmounts] = useState<Record<number, string>>({});
  const [quote, setQuote] = useState<Quote>();
  const [request, setRequest] = useState<BuildRequest>();
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [chooser, setChooser] = useState(false);
  const [tx, setTx] = useState("");
  const [txStatus, setTxStatus] = useState("");
  const [chainVersion, setChainVersion] = useState(0);
  const generation = useRef(0),
    submitting = useRef(false),
    alive = useRef(true);
  const holding = address ? omni(address) : "";
  const context =
    config && inventory ? { ...config, assets: inventory.assets } : config;
  const pending = [
    "submitted",
    "pending",
    "checking",
    "status unavailable",
  ].includes(txStatus);
  useEffect(() => {
    alive.current = true;
    const stop = discover((w) =>
      setWallets((old) =>
        old.some((x) => x.provider === w.provider) ? old : [...old, w],
      ),
    );
    api<WalletConfig>("config")
      .then((c) => {
        if (alive.current) setConfig(c);
      })
      .catch((e) => {
        if (alive.current) setError(e.message);
      });
    return () => {
      alive.current = false;
      generation.current++;
      stop();
    };
  }, []);
  function clearReview() {
    setQuote(undefined);
    setRequest(undefined);
  }
  function clearAccount() {
    generation.current++;
    setInventory(undefined);
    setHistory(undefined);
    clearReview();
    setTarget("");
    setPartial(false);
    setAmounts({});
    setError("");
    setHistoryError("");
    setBusy("");
    setTx("");
    setTxStatus("");
  }
  async function load(a: string, g = generation.current) {
    const valid = () => alive.current && g === generation.current;
    await Promise.all([
      api<WalletInventory>(`account/${a}`)
        .then((v) => {
          if (v.address !== a || v.omnilock_address !== omni(a))
            throw Error(
              "Wallet address mismatch. Refresh before transferring.",
            );
          if (valid()) setInventory(v);
        })
        .catch((e) => {
          if (valid()) setError(e.message);
        }),
      api<WalletHistory>(`transactions/${a}`)
        .then((v) => {
          if (valid()) {
            setHistory(v);
            setHistoryError("");
          }
        })
        .catch((e) => {
          if (valid()) setHistoryError(e.message);
        }),
    ]);
  }
  function useAccount(a: string) {
    clearAccount();
    setAddress(a);
    try {
      const hash = localStorage.getItem(historyKey(a));
      if (hash && /^0x[\da-f]{64}$/.test(hash)) {
        setTx(hash);
        setTxStatus("checking");
      }
    } catch {}
    void load(a);
  }
  useEffect(() => {
    if (!wallet) return;
    const changed = (accounts: unknown) => {
      if (
        Array.isArray(accounts) &&
        typeof accounts[0] === "string" &&
        /^0x[\da-f]{40}$/i.test(accounts[0])
      )
        useAccount(accounts[0].toLowerCase());
      else {
        clearAccount();
        setAddress("");
        setWallet(undefined);
      }
    };
    const disconnect = () => {
      clearAccount();
      setAddress("");
      setWallet(undefined);
    };
    const chain = () => {
      generation.current++;
      setChainVersion((v) => v + 1);
      clearReview();
      setBusy("");
      if (address) void load(address);
    };
    wallet.provider.on?.("accountsChanged", changed);
    wallet.provider.on?.("disconnect", disconnect);
    wallet.provider.on?.("chainChanged", chain);
    return () => {
      wallet.provider.removeListener?.("accountsChanged", changed);
      wallet.provider.removeListener?.("disconnect", disconnect);
      wallet.provider.removeListener?.("chainChanged", chain);
    };
  }, [wallet, address]);
  useEffect(() => {
    if (!tx || !address) return;
    let cancelled = false;
    const g = generation.current;
    const check = async () => {
      try {
        const r = await api<{ status: string | null }>(`transaction/${tx}`);
        if (cancelled || g !== generation.current) return;
        const status = r.status ?? "status unavailable";
        setTxStatus(status);
        if (status === "committed") {
          clearReview();
          await load(address, g);
        }
      } catch {
        if (!cancelled && g === generation.current)
          setTxStatus("status unavailable");
      }
    };
    void check();
    const timer = setInterval(() => void check(), 10000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [tx, address, chainVersion]);
  async function connect(w: Wallet) {
    setBusy("Connecting wallet…");
    setError("");
    const g = generation.current;
    try {
      await w.provider.request({ method: "eth_requestAccounts" });
      const a = await current(w.provider);
      if (!alive.current || g !== generation.current) return;
      setWallet(w);
      setChooser(false);
      useAccount(a);
    } catch (e) {
      if (g === generation.current) setError((e as Error).message);
    } finally {
      if (g === generation.current) setBusy("");
    }
  }
  async function refresh() {
    const g = generation.current;
    clearReview();
    setError("");
    setBusy("Refreshing mainnet assets…");
    await load(address, g);
    if (g === generation.current) setBusy("");
  }
  async function moreTransactions() {
    if (!history?.more) return;
    const g = generation.current;
    try {
      const page = await api<WalletHistory>(
        `transactions/${address}?cursor=${history.last_cursor}`,
      );
      if (g === generation.current)
        setHistory((old) =>
          old
            ? {
                ...page,
                transactions: [
                  ...old.transactions,
                  ...page.transactions.filter(
                    (t) =>
                      !old.transactions.some((x) => x.tx_hash === t.tx_hash),
                  ),
                ],
              }
            : page,
        );
    } catch (e) {
      if (g === generation.current) setHistoryError((e as Error).message);
    }
  }
  async function review() {
    if (!wallet || !context || !inventory || pending) return;
    const g = generation.current;
    clearReview();
    setError("");
    setBusy("Preparing your transfer…");
    try {
      destination(target);
      if ((await current(wallet.provider)) !== address)
        throw Error("Wallet changed. Connect again.");
      const r: BuildRequest = {
        address,
        destination: target,
        inputs: inventory.cells.map((c) => c.out_point),
        ...(partial
          ? {
              amounts: withdrawalAmounts(
                amounts,
                inventory.available,
                inventory.assets,
              ),
            }
          : {}),
      };
      const q = await api<Quote>("prepare", r);
      verifyQuote(q, r, context);
      if (g === generation.current) {
        setQuote(q);
        setRequest(r);
      }
    } catch (e) {
      if (g === generation.current) setError((e as Error).message);
    } finally {
      if (g === generation.current) setBusy("");
    }
  }
  async function transfer() {
    if (
      !wallet ||
      !quote ||
      !request ||
      !context ||
      submitting.current ||
      pending
    )
      return;
    const g = generation.current,
      q = quote,
      r = request,
      w = wallet;
    let attempted = false;
    submitting.current = true;
    setError("");
    setBusy("Confirm the signature in your ETH wallet…");
    try {
      verifyQuote(q, r, context);
      if ((await current(w.provider)) !== r.address || g !== generation.current)
        throw Error("Wallet changed. Review again.");
      const signature = await w.provider.request({
        method: "personal_sign",
        params: [q.signing_message, r.address],
      });
      if ((await current(w.provider)) !== r.address || g !== generation.current)
        throw Error("Wallet changed while signing. Nothing was submitted.");
      if (typeof signature !== "string")
        throw Error("Invalid wallet signature.");
      verifyWalletSignature(q.signing_message, signature, r.address);
      try {
        localStorage.setItem(historyKey(r.address), q.transaction_hash);
      } catch {}
      attempted = true;
      setTx(q.transaction_hash);
      setTxStatus("submitted");
      clearReview();
      setBusy("Validating and submitting on CKB…");
      const result = await api<{ transaction_hash: string }>("submit", {
        request: r,
        transaction_hash: q.transaction_hash,
        signing_message: q.signing_message,
        signature,
      });
      if (result.transaction_hash !== q.transaction_hash)
        throw Error(
          "Unexpected transaction hash. Check the reviewed transaction on Explorer.",
        );
    } catch (e) {
      const notSubmitted = (e as Error & { notSubmitted?: boolean })
        .notSubmitted;
      if (attempted && notSubmitted) {
        try {
          if (
            localStorage.getItem(historyKey(r.address)) === q.transaction_hash
          )
            localStorage.removeItem(historyKey(r.address));
        } catch {}
      }
      if (g === generation.current) {
        setError((e as Error).message);
        if (attempted && notSubmitted) {
          setTx("");
          setTxStatus("");
        } else if (attempted) setTxStatus("checking");
      }
    } finally {
      submitting.current = false;
      if (g === generation.current) setBusy("");
    }
  }
  return (
    <>
      <header>
        <a className="brand" href="/">
          <span className="brand-mark">g</span> Godwoken Refund
        </a>
        <div className="header-right">
          <span className="network">
            <i />
            CKB Mainnet
          </span>
          {address ? (
            <button
              className="wallet-button"
              onClick={() => {
                clearAccount();
                setAddress("");
                setWallet(undefined);
              }}
            >
              {short(address)} · Disconnect
            </button>
          ) : (
            <button
              className="wallet-button"
              disabled={!config || !!busy}
              onClick={() => setChooser(!chooser)}
            >
              Connect ETH wallet
            </button>
          )}
        </div>
      </header>
      <main>
        <section className="intro">
          <div className="eyebrow">YOUR ETH KEY · YOUR CKB ASSETS</div>
          <h1>Receive and withdraw your refund</h1>
          <p>
            Connect your Ethereum wallet to view your CKB address, balances and
            transactions.
          </p>
        </section>
        {chooser && (
          <section className="wallet-picker">
            <div className="section-heading">
              <h2>Connect an Ethereum wallet</h2>
              <button onClick={() => setChooser(false)}>Close</button>
            </div>
            {wallets.length ? (
              wallets.map((w) => (
                <button
                  key={w.id}
                  disabled={!!busy}
                  onClick={() => void connect(w)}
                >
                  {w.name}
                  <span>Connect →</span>
                </button>
              ))
            ) : (
              <p>
                Open this page in a browser with MetaMask, Rabby, or another
                Ethereum wallet.
              </p>
            )}
            <small>
              Connecting reads your public address. Your private key stays in
              your wallet.
            </small>
          </section>
        )}
        {error && (
          <div className="alert" role="alert">
            {error}
          </div>
        )}
        {busy && (
          <div className="progress" role="status">
            <span className="spinner" />
            {busy}
          </div>
        )}
        {address && (
          <section className="card receive-card">
            <h2>Your refund receiving address</h2>
            <p className="holding-address">{holding}</p>
            <div className="receive-actions">
              <button
                onClick={() =>
                  void navigator.clipboard
                    .writeText(holding)
                    .catch(() => setError("Select and copy the address above."))
                }
              >
                Copy address
              </button>
              <a
                href={`https://explorer.nervos.org/address/${holding}`}
                target="_blank"
                rel="noreferrer"
              >
                View assets and all transactions on Nervos Explorer ↗
              </a>
            </div>
            <p>
              Use this address for CKB mainnet transfers. Your connected ETH key
              controls it.
            </p>
          </section>
        )}
        <div className="workspace">
          <section className="card assets-card">
            <div className="card-heading">
              <h2>Your assets</h2>
              {address && (
                <button
                  className="text-button"
                  disabled={!!busy}
                  onClick={() => void refresh()}
                >
                  Refresh
                </button>
              )}
            </div>
            {!address ? (
              <div className="empty">
                <h3>Connect your ETH wallet</h3>
                <p>Your receiving address appears as soon as you connect.</p>
                <button
                  className="primary"
                  disabled={!config}
                  onClick={() => setChooser(true)}
                >
                  Connect ETH wallet
                </button>
              </div>
            ) : !inventory ? (
              <div className="empty">
                <p>Loading mainnet balances…</p>
              </div>
            ) : (
              <>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>Asset</th>
                        <th>On-chain balance</th>
                        <th>Transferable</th>
                      </tr>
                    </thead>
                    <tbody>
                      {inventory.assets.map((a) => (
                        <tr key={a.asset_id}>
                          <td>
                            <strong>
                              {a.symbol ?? `sUDT ${short(a.type_hash)}`}
                            </strong>
                            {a.asset_id !== 1 && (
                              <small>
                                <a
                                  href={`https://explorer.nervos.org/sudt/${a.type_hash}`}
                                  target="_blank"
                                  rel="noreferrer"
                                >
                                  Token details ↗
                                </a>
                              </small>
                            )}
                          </td>
                          <td>
                            {display(
                              inventory.balances.find(
                                (b) => b.asset_id === a.asset_id,
                              )?.raw ?? "0",
                              a.l1_decimals,
                            )}
                          </td>
                          <td>
                            {display(
                              inventory.transferable.find(
                                (b) => b.asset_id === a.asset_id,
                              )?.raw ?? "0",
                              a.l1_decimals,
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <p className="table-note">
                  CKB includes capacity held in token cells. Standard sUDTs with
                  unknown metadata are shown in exact raw units.
                </p>
                {inventory.more_batches && (
                  <p className="inline-note">
                    This transfer uses the next 64 cells. Refresh after
                    confirmation to transfer the rest.
                  </p>
                )}
                {inventory.confirming_cells > 0 && (
                  <p className="inline-note">
                    {inventory.confirming_cells} cells await{" "}
                    {inventory.confirmations_required} confirmation(s).
                  </p>
                )}
                {inventory.other_cells.length > 0 && (
                  <details>
                    <summary>
                      Other on-chain assets ({inventory.other_cells.length}{" "}
                      cells)
                    </summary>
                    <p>
                      These cells need their asset-specific transfer tools.
                      Their CKB capacity is included above.
                    </p>
                    {inventory.other_cells.map((c) => (
                      <p key={`${c.out_point.tx_hash}:${c.out_point.index}`}>
                        <a
                          href={explorerTx(c.out_point.tx_hash)}
                          target="_blank"
                          rel="noreferrer"
                        >
                          {short(
                            c.output.type?.code_hash ?? c.out_point.tx_hash,
                          )}{" "}
                          · cell {c.out_point.index} ↗
                        </a>
                        <code className="holding-address">{c.output_data}</code>
                      </p>
                    ))}
                  </details>
                )}
              </>
            )}
          </section>
          <aside className="card transfer-card">
            <h2>Withdraw assets</h2>
            <p>Sign with your ETH wallet. The network fee is paid in CKB.</p>
            <label htmlFor="destination">Destination CKB address</label>
            <textarea
              id="destination"
              placeholder="ckb1…"
              spellCheck={false}
              value={target}
              disabled={!inventory?.cells.length || !!busy || pending}
              onChange={(e) => {
                setTarget(e.target.value);
                clearReview();
              }}
            />
            <fieldset
              className="withdraw-amounts"
              disabled={!inventory?.cells.length || !!busy || pending}
            >
              <legend>Amount</legend>
              <label>
                <input
                  type="checkbox"
                  checked={partial}
                  onChange={(e) => {
                    setPartial(e.target.checked);
                    clearReview();
                  }}
                />{" "}
                Choose amounts
              </label>
              {!partial && (
                <p>Withdraw all assets in this batch, minus the network fee.</p>
              )}
              {partial && inventory && (
                <>
                  <p className="field-help">
                    Include enough CKB for recipient token cells and change. The
                    fee is additional.
                  </p>
                  {inventory.available.map((b) => {
                    const a = inventory.assets.find(
                      (a) => a.asset_id === b.asset_id,
                    )!;
                    return (
                      <label key={b.asset_id}>
                        {a.symbol ?? `sUDT ${short(a.type_hash)}`}
                        {a.l1_decimals == null ? " (raw units)" : ""}
                        <input
                          type="text"
                          inputMode="decimal"
                          placeholder="0"
                          value={amounts[b.asset_id] ?? ""}
                          onChange={(e) => {
                            setAmounts((old) => ({
                              ...old,
                              [b.asset_id]: e.target.value,
                            }));
                            clearReview();
                          }}
                        />
                        <span className="field-help">
                          Available in this batch:{" "}
                          {display(b.raw, a.l1_decimals)}
                        </span>
                      </label>
                    );
                  })}
                </>
              )}
            </fieldset>
            <div className="transfer-facts">
              <div>
                <span>Network fee</span>
                <strong>
                  {quote
                    ? `${display(quote.fee_shannons, 8)} CKB`
                    : "Shown before signing"}
                </strong>
              </div>
              <div>
                <span>Service fee</span>
                <strong>0 CKB</strong>
              </div>
            </div>
            {!quote ? (
              <button
                className="primary full"
                disabled={
                  !inventory?.cells.length || !target || !!busy || pending
                }
                onClick={() => void review()}
              >
                Review withdrawal →
              </button>
            ) : (
              <div className="review">
                <h3>Review before signing</h3>
                <p className="recipient">{quote.destination}</p>
                <p>
                  CKB received:{" "}
                  <strong>
                    {display(quote.received_capacity_shannons, 8)}
                  </strong>
                </p>
                {context &&
                  transferTokens(quote, context).map((t) => (
                    <p key={t.asset.asset_id}>
                      {t.asset.symbol ?? `sUDT ${short(t.asset.type_hash)}`}:{" "}
                      <strong>{display(t.raw, t.asset.l1_decimals)}</strong>
                    </p>
                  ))}
                <p>Network fee: {display(quote.fee_shannons, 8)} CKB</p>
                <button
                  className="primary full"
                  disabled={!!busy || !config?.broadcast_enabled || pending}
                  onClick={() => void transfer()}
                >
                  Sign & withdraw
                </button>
                <button
                  className="text-button full"
                  disabled={!!busy}
                  onClick={clearReview}
                >
                  Back to edit
                </button>
              </div>
            )}
            {config && !config.broadcast_enabled && (
              <p className="operator-note">
                Submission is disabled on this service.
              </p>
            )}
            {tx && (
              <div className="receipt" role="status">
                <strong>Transaction: {txStatus}</strong>
                <p>
                  <a href={explorerTx(tx)} target="_blank" rel="noreferrer">
                    {short(tx)} · View on Explorer ↗
                  </a>
                </p>
                <p>Check this transaction before sending again.</p>
                {["committed", "rejected"].includes(txStatus) && (
                  <button
                    onClick={() => {
                      try {
                        localStorage.removeItem(historyKey(address));
                      } catch {}
                      setTx("");
                      setTxStatus("");
                      void refresh();
                    }}
                  >
                    New withdrawal
                  </button>
                )}
              </div>
            )}
          </aside>
        </div>
        {address && (
          <section className="card transactions-card">
            <div className="card-heading">
              <h2>Transactions</h2>
              <a
                href={`https://explorer.nervos.org/address/${holding}`}
                target="_blank"
                rel="noreferrer"
              >
                View all on Nervos Explorer ↗
              </a>
            </div>
            {historyError && (
              <p role="alert">Could not load transactions: {historyError}</p>
            )}
            {history && !history.transactions.length && (
              <p>No confirmed transactions yet.</p>
            )}
            {history?.transactions.map((t) => (
              <div className="transaction-row" key={t.tx_hash}>
                <a
                  href={explorerTx(t.tx_hash)}
                  target="_blank"
                  rel="noreferrer"
                >
                  {short(t.tx_hash)} ↗
                </a>
                <span>
                  {t.cells.some((c) => c[0] === "input")
                    ? "Sent / change"
                    : "Received"}
                </span>
                <span>Block {BigInt(t.block_number).toString()}</span>
              </div>
            ))}
            {history?.more && (
              <button onClick={() => void moreTransactions()}>
                Load more transactions
              </button>
            )}
          </section>
        )}
      </main>
      <footer>
        <span>Godwoken Refund · CKB Mainnet</span>
        <span>Signed by your ETH key. No ETH gas.</span>
      </footer>
    </>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
