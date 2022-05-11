# Layer2 account

Each layer2 account is conceptually a tuple of `(ID, L1 script)`:

* ID is an auto-increment number.
* L1 script, a CKB core data structure, unique for each layer2 account.

The `Script` is defined as follows which is also a CKB core data structure:

```
/* https://github.com/nervosnetwork/ckb/blob/develop/util/types/schemas/blockchain.mol */
table Script {
    code_hash:      Byte32,
    hash_type:      byte,
    args:           Bytes,
}
```

CKB uses this structure to indicate which script to load when verifying an L1 transaction. On Layer2, we reuse the structure:

* code_hash - code_hash pointing to an executable binary that verifies a layer 2 transaction. For example: if a layer2 account represents an EVM contract, its `script.code_hash` points to the binary that can verify the EVM transaction.
* hash_type - hash_type affects how CKB loads the script binary, in layer2 account, it is fixed to `HashType::Type`.
* args - args is used to set script initial args. We set the first 32 bytes to `rollup_script_hash` to distinguish accounts of different rollups.

Whether a layer2 account is an EOA or a contract is determined by its `code_hash`. If the script assumes the account is always a receiver of transactions then the account is a contract, or if the script assumes the account is always a sender of transactions then the account is an EOA. The script's code should perform some contract logic check or signature check based on the assumption.

## Account alias

We introduced the registry and Registry address since [Godwoken v1](https://github.com/nervosnetwork/godwoken/blob/develop/docs/v1-release-note.md). Registry and Registry addresses can be seen as an alias mechanism for layer 2 accounts.

A layer2 account itself can be referenced by ID or script(normally, hash of the script). However, we also need to access an account by alias name in some environments, for example, in the EVM environment we have no concept of Godwoken's `ID` or `script`, we can only access an account by ETH address.

A registry itself is an account. The mapping relation of aliases is stored in the registry account's key-value storage.

The registry address is encoded as follows:

```
(registry ID 4 bytes) | (alias address length 4 bytes) | (alias address n bytes)
```

Currently in Godwoken v1, we only implement one registry - the ETH registry, for accounts created on Godwoken through deposit we automatically build an ETH address alias for the account.
