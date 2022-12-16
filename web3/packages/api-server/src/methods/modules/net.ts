import { HexNumber } from "@ckb-lumos/base";
import { isListening } from "../../app/app";
import { gwConfig } from "../../base/index";

export class Net {
  constructor() {}

  /**
   * Returns the current net version
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * net version as the second argument
   */
  version(_args: []): string {
    return BigInt(gwConfig.web3ChainId).toString(10);
  }

  /**
   * Returns the current peer nodes number, which is always 0 since godwoken is not implementing p2p network
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * current peer nodes number as the second argument
   */
  peerCount(_args: []): HexNumber {
    return "0x0";
  }

  /**
   * Returns if the client is currently listening
   * @param  {Array<*>} [params] An empty array
   * @param  {Function} [cb] A function with an error object as the first argument and the
   * boolean as the second argument
   */
  listening(_args: []): boolean {
    return isListening();
  }
}
