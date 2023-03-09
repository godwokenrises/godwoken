import { HexString } from "@ckb-lumos/base";
import { Uint32 } from "./types/uint";

// NOTE: Not to import `gwConfig`
export class BaseEthRegistryAddress {
  private registryId: number;
  private addressByteSize: number = 20;
  public readonly address: HexString;

  // Using optional registryId when gwConfig not initialized
  constructor(address: HexString, registryId: number) {
    if (!address.startsWith("0x") || address.length !== 42) {
      throw new Error(`Eth address format error: ${address}`);
    }
    this.address = address.toLowerCase();

    this.registryId = registryId;
  }

  public serialize(): HexString {
    return (
      "0x" +
      new Uint32(this.registryId).toLittleEndian().slice(2) +
      new Uint32(this.addressByteSize).toLittleEndian().slice(2) +
      this.address.slice(2)
    );
  }

  public static Deserialize(hex: HexString): BaseEthRegistryAddress {
    const hexWithoutPrefix = hex.slice(2);
    const registryId: number = Uint32.fromLittleEndian(
      hexWithoutPrefix.slice(0, 8)
    ).getValue();
    const addressByteSize: number = Uint32.fromLittleEndian(
      hexWithoutPrefix.slice(8, 16)
    ).getValue();
    const address: HexString = hexWithoutPrefix.slice(16);
    if (addressByteSize !== 20 || address.length !== 40) {
      throw new Error(`Eth address deserialize error: ${hex}`);
    }
    return new BaseEthRegistryAddress("0x" + address, registryId);
  }
}
