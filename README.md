# Godwoken

> Godwoken v1 release note: [docs/release-notes/v1-release-note.md](https://github.com/nervosnetwork/godwoken/blob/develop/docs/release-notes/v1-release-note.md)

Godwoken is an optimistic rollup solution builtin upon [Nervos CKB](https://docs.nervos.org/). It is designed to be configurable in many regards, and continues to evolution:

* In the current implementation, a fixed group of block producers can issue new layer2 blocks. In the future we plan for a Proof of Stake solution, where we can evolve the block producing to the decentralized processing.
* Right now [polyjuice](https://github.com/nervosnetwork/godwoken-polyjuice) is integrated to godwoken for an Ethereum compatible solution. However, godwoken at its core only provides a flexible [programming interface](https://github.com/nervosnetwork/godwoken-scripts/blob/master/c/gw_def.h). A result of this, is that any account model based blockchain model can be integrated with godwoken this way. Similar to polyjuice on godwoken, we could also have EOS on godwoken, Libra on godwoken, etc.
