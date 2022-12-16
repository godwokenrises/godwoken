import { HexNumber, HexString } from "@ckb-lumos/base";
import { GodwokenClient } from "@godwoken-web3/godwoken";
import { Uint32 } from "../base/types/uint";

export class EntryPointContract {
  public readonly address: HexString;
  private iAccountId: number | undefined;
  private rpc: GodwokenClient;
  private registryAccountId: HexNumber;

  constructor(
    rpcOrUrl: GodwokenClient | string,
    address: HexString,
    registryAccountId: HexNumber
  ) {
    if (typeof rpcOrUrl === "string") {
      this.rpc = new GodwokenClient(rpcOrUrl);
    } else {
      this.rpc = rpcOrUrl;
    }

    this.address = address;
    this.registryAccountId = registryAccountId;
  }

  async init() {
    const registry =
      "0x" +
      new Uint32(+this.registryAccountId).toLittleEndian().slice(2) +
      new Uint32(20).toLittleEndian().slice(2) +
      this.address.slice(2);

    const scriptHash = await this.rpc.getScriptHashByRegistryAddress(registry);
    if (scriptHash == null) {
      throw new Error(
        `script hash not found by registry(${registry}) from entrypoint address(${this.address})`
      );
    }

    const accountId = await this.rpc.getAccountIdByScriptHash(scriptHash);
    if (accountId == null) {
      throw new Error(
        `account id not found by script hash(${scriptHash}) from entrypoint address(${this.address})`
      );
    }

    this.iAccountId = accountId;
  }

  public get accountId(): number {
    return this.iAccountId!;
  }
}
