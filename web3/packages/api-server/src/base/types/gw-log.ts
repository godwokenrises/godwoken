import { HexString, Hash, HexNumber } from "@ckb-lumos/base";

export interface PolyjuiceUserLog {
  address: HexString;
  data: HexString;
  topics: Hash[];
}

export interface PolyjuiceSystemLog {
  gasUsed: HexNumber;
  cumulativeGasUsed: HexNumber;
  createdAddress: HexString;
  statusCode: HexNumber;
}
