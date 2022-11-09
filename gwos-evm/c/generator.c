/* Layer 2 contract generator
 *
 * The generator supposed to be run off-chain.
 * generator dynamic linking with the layer2 contract code,
 * and provides layer2 syscalls.
 *
 * A program should be able to generate a post state after run the generator,
 * and should be able to use the states to construct a transaction that satisfies
 * the validator.
 */

#define GW_GENERATOR

#include "polyjuice.h"

int main() { return run_polyjuice(); }
