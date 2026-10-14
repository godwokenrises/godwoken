import type { Config, Quote } from "./types";
export type Protocol = "v1" | "v0";
export type ScopedConfig = Config & { protocol: Protocol };
export type ScopedQuote = Quote & { protocol: Protocol; plan_manifest: string };
export const endpoint = (protocol: Protocol, path: string) => `/api/${protocol}/${path}`;
export const historyKey = (protocol: Protocol, manifest: string, owner: string) =>
  `gw-withdraw:${protocol}:${manifest}:${owner.toLowerCase()}`;
export function verifyScope(value: { protocol: Protocol; plan_manifest: string }, protocol: Protocol, manifest: string) {
  if (value.protocol !== protocol || value.plan_manifest !== manifest)
    throw new Error("Refund protocol or release changed. Reload and review a new transfer.");
}
