# Project layout

* `c/`
  * generator.c - an off-chain script to generate new states
  * validator.c - an on-chain script to validate state transitions
  * sudt.c - an layer2 contract to represent Simple UDT assets
* `contracts/`
  * state-validator - the main Rollup contract to validate the state transition
  * deposition-lock - the lock script used for deposition cells
* `crates/`
  * chain - receive data from lumos and maintaining the layer2 chain
  * generator - a wrapper of `c/generator.c`, used to generate new states offchain
  * common - a common crate that used by both on-chain and off-chain
  * config - configurations
  * setup-tool - a tool to generate Godwoken configurations

Notice all crates are started with `gw-` prefix as a naming convention.
