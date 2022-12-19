# Godwoken

> Godwoken v1 release note: [docs/v1-release-note.md](https://github.com/nervosnetwork/godwoken/blob/develop/docs/v1-release-note.md)

Godwoken is an optimistic roll-up solution built upon the [Nervos CKB](https://docs.nervos.org/). It is designed to be configurable in many regards and continues to evolve:

* In the current implementation, a fixed group of block producers can issue new layer 2 blocks. In the future, we plan for a Proof of Stake solution, where we can evolve the block producing to the decentralized processing.
* Currently, [polyjuice](https://github.com/nervosnetwork/godwoken-polyjuice) is integrated into Godwoken for an Ethereum compatible solution. However, Godwoken, at its core, only provides a flexible [programming interface](https://github.com/nervosnetwork/godwoken-scripts/blob/master/c/gw_def.h). A result of this is that any account-based blockchain model can be integrated with Godwoken this way. Similar to Polyjuice on Godwoken, we could also have EOS on Godwoken, Libra on Godwoken, etc.

## Documentation

Please follow this [documentation](https://docs.godwoken.io) to learn more about Godwoken.
