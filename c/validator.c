/* Layer 1 validator contract
 *
 * Verify:
 *  1. The kv state changes are valid
 *     - verify old state
 *     - verify new state
 *  2. The entrance account script is valid (lazy, verify when load account
 * script)
 *  3. Verify new accounts
 *  4. Verify return data: hash(return_data) == return_data_hash
 */

#define GW_VALIDATOR

#include "polyjuice.h"

int main() { return run_polyjuice(); }
