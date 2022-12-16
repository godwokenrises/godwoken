# ETH Compatibility

## RPC compatibility

### 1. ZERO ADDRESS

Godwoken does not have the corresponding "zero address"(0x0000000000000000000000000000000000000000) concept, so Polyjuice won't be able to handle zero address as well.

#### Result

Transaction with zero address in from/to filed is not supported.

known issue: #246

#### Recommend workaround

- if you are trying to use zero address as a black hole to burn ethers, you can use `transfer function` in `CKB_ERC20_Proxy` to send ethers to zero address. more info can be found in the above section `Transfer Value From EOA To EOA`.

### 2. GAS LIMIT

Godwoken limit the transaction execution resource in CKB-VM with [Cycle Limit](https://docs-xi-two.vercel.app/docs/rfcs/0014-vm-cycle-limits/0014-vm-cycle-limits), we set the `RPC_GAS_LIMIT` to `50000000` for max compatibility with Ethereum toolchain, but the real gas limit you can use depends on such Cycle Limit.

## EVM compatibility

- [Godwoken-Polyjuice](https://github.com/nervosnetwork/godwoken-polyjuice/blob/compatibility-breaking-changes/docs/EVM-compatible.md)
