import { describe, it, expect } from "vitest";
import { endpoint, historyKey, verifyScope } from "../src/scope";
describe("explicit protocol and release isolation", () => {
  it("uses separate endpoints and history for the same ETH owner", () => {
    const owner = "0x" + "12".repeat(20);
    expect(endpoint("v0", "prepare")).not.toBe(endpoint("v1", "prepare"));
    expect(historyKey("v0", "root", owner)).not.toBe(historyKey("v1", "root", owner));
    expect(historyKey("v1", "old", owner)).not.toBe(historyKey("v1", "new", owner));
  });
  it("rejects cross-version quotes even when owner and assets coincide", () => {
    expect(() => verifyScope({ protocol: "v0", plan_manifest: "root" }, "v1", "root")).toThrow();
    expect(() => verifyScope({ protocol: "v1", plan_manifest: "old" }, "v1", "new")).toThrow();
    expect(() => verifyScope({ protocol: "v0", plan_manifest: "root" }, "v0", "root")).not.toThrow();
  });
});
