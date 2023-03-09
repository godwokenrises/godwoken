import { Hash, HexString, Script, utils } from "@ckb-lumos/base";
import { GodwokenClient } from "@godwoken-web3/godwoken";
import { Store } from "../cache/store";
import { BaseEthRegistryAddress } from "./base-eth-registry-address";
import { gwConfig } from "./index";
import { logger } from "./logger";

// the eth address vs script hash is not changeable, so we set no expire for cache
const scriptHashCache = new Store(false);

// Only support eth address now!
export class EthRegistryAddress extends BaseEthRegistryAddress {
  // Using optional registryId when gwConfig not initialized
  constructor(
    address: HexString,
    { registryId }: { registryId?: number } = {}
  ) {
    super(address, registryId || +gwConfig.accounts.ethAddrReg.id);
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
