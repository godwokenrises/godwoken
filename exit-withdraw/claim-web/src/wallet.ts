import type { Provider, Wallet } from "./types";
export function discover(onWallet: (wallet: Wallet) => void) {
  const announce = (event: Event) => {
    const d = (event as CustomEvent).detail;
    if (
      d?.provider?.request &&
      typeof d?.info?.uuid === "string" &&
      typeof d?.info?.name === "string"
    )
      onWallet({
        id: d.info.uuid,
        name: d.info.name.slice(0, 80),
        provider: d.provider,
      });
  };
  window.addEventListener("eip6963:announceProvider", announce);
  window.dispatchEvent(new Event("eip6963:requestProvider"));
  const timer = window.setTimeout(() => {
    const eth = window.ethereum;
    const providers = eth?.providers ?? (eth ? [eth] : []);
    providers.forEach((p, i) =>
      onWallet({
        id: `injected-${i}`,
        name: p.isRabby
          ? "Rabby"
          : p.isCoinbaseWallet
            ? "Coinbase Wallet"
            : p.isMetaMask
              ? "MetaMask"
              : "Ethereum wallet",
        provider: p,
      }),
    );
  }, 300);
  return () => {
    window.removeEventListener("eip6963:announceProvider", announce);
    window.clearTimeout(timer);
  };
}
export async function current(provider: Provider) {
  const a = await provider.request({ method: "eth_accounts" });
  if (
    !Array.isArray(a) ||
    typeof a[0] !== "string" ||
    !/^0x[\da-f]{40}$/i.test(a[0])
  )
    throw new Error("Connect an Ethereum account to continue.");
  return a[0].toLowerCase();
}
