#include "test_utils.h"
#include "../../gwos/c/gw_def.h"
#include "../../gwos/c/generator_utils.h"
#include "../polyjuice_utils.h"
#include <assert.h>

void test(uint128_t gas_price, uint64_t gas_used, uint256_t expected_fee) {
  uint256_t result = calculate_fee(gas_price, gas_used);
  assert(gw_uint256_cmp(result, expected_fee) == GW_UINT256_EQUAL);
}

int main() {
  uint256_t expected_result = {0};
  gw_uint256_cmp(expected_result, expected_result);
  test(0, 0, expected_result);
  test(0, 1, expected_result);
  test(1, 0, expected_result);

  gw_uint256_one(&expected_result);
  test(1, 1, expected_result);

  uint128_t gas_price = 11;
  expected_result.array[0] = 22;
  test(gas_price, 2, expected_result);

  gas_price = 0xfedbca9876543210ULL;
  expected_result.array[0] = 0x76543210;
  expected_result.array[1] = 0xfedbca98;
  test(gas_price, 1, expected_result);
  test(gas_price, 2, {0xECA86420UL, 0xFDB79530UL, 0x1UL});

  gas_price = ((uint128_t)0xF0F0F0F0F0F0F0F0 << 64) | 0xF0F0F0F0F0F0F0F0;
  gw_uint256_zero(&expected_result);
  test(gas_price, 0, expected_result);
  test(gas_price, 1, {0xF0F0F0F0UL, 0xF0F0F0F0UL, 0xF0F0F0F0UL, 0xF0F0F0F0UL});

  uint64_t gas_used = 0xaaaaaaaaaaaaaaaaULL;
  test(gas_price, gas_used, {0x5f5f5f60, 0x5f5f5f5f, 0xffffffff, 0xffffffff, 
                             0xA0A0A09F, 0xA0A0A0A0, 0x00000000, 0x00000000});

  const uint64_t MAX_UINT64 = 0xFFFFFFFFFFFFFFFF;
  gas_used = MAX_UINT64;
  gas_price = ((uint128_t)MAX_UINT64 << 64) | MAX_UINT64;
  test(gas_price, gas_used, {0x00000001, 0x00000000, 0xffffffff, 0xffffffff, 
                             0xFFFFFFFE, 0xFFFFFFFF, 0x00000000, 0x00000000});

  return 0;
}
