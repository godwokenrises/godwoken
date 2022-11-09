
## Addition Features
* pre-compiled contracts
  - Add `recover_account` to recover any supported signature
  - Add `balance_of_any_sudt` to query the balance of any sudt_id account
  - Add `transfer_to_any_sudt` to transfer value by sudt_id (Must collaborate with SudtERC20Proxy_UserDefinedDecimals.sol contract)

### `recover_account` Spec

```
  Recover an EoA account script hash by signature

  input: (the input data is from abi.encode(mesage, signature, code_hash))
  ======
    input[ 0..32]  => message
    input[32..64]  => offset of signature part
    input[64..96]  => code_hash (EoA lock hash)
    input[96..128] => length of signature data
    input[128..]   => signature data

  output (32 bytes):
  =======
    output[0..32] => account script hash
```

See: [Example](../polyjuice-tests/src/test_cases/evm-contracts/RecoverAccount.sol)

### `balance_of_any_sudt` Spec

```
  Query the balance of `account_id` of `sudt_id` token.

   input:
   ======
     input[ 0..32] => sudt_id (big endian)
     input[32+12..64] => address (eth_address)

   output:
   =======
     output[0..32] => amount
```

See: [Example](../solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.sol)

### `transfer_to_any_sudt` Spec

```
  Transfer `sudt_id` token from `from_id` to `to_id` with `amount` balance.

  NOTE: This pre-compiled contract need caller to check permission of `from_id`,
  currently only `solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.sol` is allowed to call this contract.

   input:
   ======
     input[ 0..32 ] => sudt_id (big endian)
     input[32+12..64 ] => from_addr (eth address)
     input[64+12..96 ] => to_addr (eth address)
     input[96..128] => amount (big endian)

   output: []
```

See: [Example](../solidity/erc20/SudtERC20Proxy_UserDefinedDecimals.sol)
