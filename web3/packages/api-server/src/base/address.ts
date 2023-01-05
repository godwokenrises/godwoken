import { Hash, HexString, Script, utils } from "@ckb-lumos/base";
import { GodwokenClient } from "@godwoken-web3/godwoken";
import { Store } from "../cache/store";
import { COMPATIBLE_DOCS_URL } from "../methods/constant";
import { gwConfig } from "./index";
import { logger } from "./logger";
import { Uint32 } from "./types/uint";

const ZERO_ETH_ADDRESS = "0x" + "00".repeat(20);

// the eth address vs script hash is not changeable, so we set no expire for cache
const scriptHashCache = new Store(false);

// Only support eth address now!
export class EthRegistryAddress {
  private registryId: number = +gwConfig.accounts.ethAddrReg.id;
  private addressByteSize: number = 20;
  public readonly address: HexString;

  constructor(address: HexString) {
    if (!address.startsWith("0x") || address.length != 42) {
      throw new Error(`Eth address format error: ${address}`);
    }
    this.address = address.toLowerCase();
  }

  public serialize(): HexString {
    return (
      "0x" +
      new Uint32(this.registryId).toLittleEndian().slice(2) +
      new Uint32(this.addressByteSize).toLittleEndian().slice(2) +
      this.address.slice(2)
    );
  }

  public static Deserialize(hex: HexString): EthRegistryAddress {
    const hexWithoutPrefix = hex.slice(2);
    // const registryId: number = Uint32.fromLittleEndian(hexWithoutPrefix.slice(0, 8)).getValue();
    const addressByteSize: number = Uint32.fromLittleEndian(
      hexWithoutPrefix.slice(8, 16)
    ).getValue();
    const address: HexString = hexWithoutPrefix.slice(16);
    if (addressByteSize !== 20 || address.length !== 40) {
      throw new Error(`Eth address deserialize error: ${hex}`);
    }
    return new EthRegistryAddress("0x" + address);
  }
}

export async function ethAddressToScriptHash(
  ethAddress: HexString,
  godwokenClient: GodwokenClient
): Promise<Hash | undefined> {
  // try get result from redis cache
  const CACHE_KEY_PREFIX = "ethAddressToScriptHash";
  let result = await scriptHashCache.get(`${CACHE_KEY_PREFIX}:${ethAddress}`);
  if (result != null) {
    logger.debug(
      `[ethAddressToScriptHash] using cache: ${ethAddress} -> ${result}`
    );
    return result;
  }

  const registryAddress: EthRegistryAddress = new EthRegistryAddress(
    ethAddress
  );
  const scriptHash: Hash | undefined =
    await godwokenClient.getScriptHashByRegistryAddress(
      registryAddress.serialize()
    );

  // add cache
  if (scriptHash != null) {
    logger.debug(
      `[ethAddressToScriptHash] update cache: ${ethAddress} -> ${scriptHash}`
    );
    scriptHashCache.insert(`${CACHE_KEY_PREFIX}:${ethAddress}`, scriptHash);
  }

  return scriptHash;
}

export async function ethAddressToAccountId(
  ethAddress: HexString,
  godwokenClient: GodwokenClient
): Promise<number | undefined> {
  if (ethAddress === "0x") {
    return +gwConfig.accounts.polyjuiceCreator.id;
  }

  if (ethAddress === ZERO_ETH_ADDRESS) {
    throw new Error(
      `zero address ${ZERO_ETH_ADDRESS} has no valid account_id! more info: ${COMPATIBLE_DOCS_URL}`
    );
  }

  const scriptHash: Hash | undefined = await ethAddressToScriptHash(
    ethAddress,
    godwokenClient
  );
  if (scriptHash == null) {
    return undefined;
  }

  const id: number | undefined = await godwokenClient.getAccountIdByScriptHash(
    scriptHash
  );
  return id;
}

export function ethEoaAddressToScriptHash(address: string) {
  const script: Script = {
    code_hash: gwConfig.eoaScripts.eth.typeHash,
    hash_type: "type",
    args: gwConfig.rollupCell.typeHash + address.slice(2),
  };
  const scriptHash = utils.computeScriptHash(script);
  return scriptHash;
}
