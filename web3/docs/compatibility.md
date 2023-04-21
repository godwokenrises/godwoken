# ETH Compatibility

## RPC compatibility

### 1. GAS LIMIT

Godwoken limit the transaction execution resource in CKB-VM with [Cycle Limit](https://docs-xi-two.vercel.app/docs/rfcs/0014-vm-cycle-limits/0014-vm-cycle-limits), we set the `RPC_GAS_LIMIT` to `50000000` for max compatibility with Ethereum toolchain, but the real gas limit you can use depends on such Cycle Limit.

## EVM compatibility

- [Godwoken-Polyjuice](../../gwos-evm/docs/EVM-compatible.md)
