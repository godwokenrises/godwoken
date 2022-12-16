import web3Utils from "web3-utils";
import { envConfig } from "./env-config";
import { Decimal } from "decimal.js";
import { parseFixed } from "@ethersproject/bignumber";

// we enlarger it to be an integer instead of float
const LOWER_CKB_PRICE = enlargeCkbPrice("0.0038");
const UPPER_GAS_PRICE = web3Utils.toWei("0.00002", "ether");
const DEFAULT_GAS_PRICE_DIVIDER =
  BigInt(UPPER_GAS_PRICE) * BigInt(LOWER_CKB_PRICE);

// when ckbPrice goes up, the gasPrice goes down (vice versa)
//   gasPrice = divider / ckbPrice
const GAS_PRICE_DIVIDER = envConfig.gasPriceDivider
  ? BigInt(envConfig.gasPriceDivider)
  : DEFAULT_GAS_PRICE_DIVIDER;

// feeRate = gasPrice * multiplier
export const FEE_RATE_MULTIPLIER = BigInt(100);

// upper-limit and lower-limit to prevent gas-price goes off-track
export const MIN_GAS_PRICE_LOWER_LIMIT = pCKBToWei(
  envConfig.minGasPriceLowerLimit || "0.00001"
);
export const MIN_GAS_PRICE_UPPER_LIMIT = pCKBToWei(
  envConfig.minGasPriceUpperLimit || "0.00004"
);

export class Price {
  private ckbPrice: string;

  constructor(ckbPrice: string) {
    this.ckbPrice = ckbPrice;
  }

  toGasPrice(): bigint {
    const ckbPrice = enlargeCkbPrice(this.ckbPrice);
    const gasPrice = GAS_PRICE_DIVIDER / ckbPrice;
    return gasPrice;
  }

  toMinGasPrice(): bigint {
    const p = this.toGasPrice();
    if (p > MIN_GAS_PRICE_UPPER_LIMIT) return MIN_GAS_PRICE_UPPER_LIMIT;
    if (p < MIN_GAS_PRICE_LOWER_LIMIT) return MIN_GAS_PRICE_LOWER_LIMIT;
    return p;
  }

  toFeeRate(): bigint {
    return FEE_RATE_MULTIPLIER * this.toGasPrice();
  }

  toMinFeeRate(): bigint {
    return FEE_RATE_MULTIPLIER * this.toMinGasPrice();
  }

  public static from(ckbPrice: string): Price {
    return new Price(ckbPrice);
  }
}

//*** helper function ***/
function enlargeCkbPrice(price: string): bigint {
  // 0.000000 => 6 precision
  // enlarge it to 10 ** 6
  const precision = 6;
  const p = new Decimal(price).toFixed(precision);
  return parseFixed(p, precision).toBigInt();
}

function pCKBToWei(pCKB: string): bigint {
  return BigInt(web3Utils.toWei(pCKB, "ether"));
}
