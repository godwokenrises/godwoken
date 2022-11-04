#ifndef GW_UINT256_H_
#define GW_UINT256_H_

#define MAX_UINT32 ((uint64_t)0xFFFFFFFF)

typedef struct uint256_t {
  uint32_t array[8];
} uint256_t;

void gw_uint256_zero(uint256_t* num) {
  for (int i = 0; i < 8; ++i) {
    num->array[i] = 0;
  }
}

void gw_uint256_one(uint256_t* num) {
  gw_uint256_zero(num);
  num->array[0] = 1;
}

void gw_uint256_max(uint256_t* num) {
  for (int i = 0; i < 8; ++i) {
    num->array[i] = (uint32_t)0xFFFFFFFF;
  }
}

int gw_uint256_overflow_add(const uint256_t a, const uint256_t b,
                            uint256_t* sum) {
  uint64_t tmp;

  int carry = 0;
  gw_uint256_zero(sum);

  for (int i = 0; i < 8; ++i) {
    tmp = (uint64_t)a.array[i] + b.array[i] + carry;
    carry = (tmp > MAX_UINT32);
    sum->array[i] = (tmp & MAX_UINT32);
  }

  return carry;
}

int gw_uint256_underflow_sub(const uint256_t a, const uint256_t b,
                             uint256_t* rem) {
  uint64_t res;
  uint64_t tmp1;
  uint64_t tmp2;

  int borrow = 0;
  gw_uint256_zero(rem);

  for (int i = 0; i < 8; ++i) {
    tmp1 = (uint64_t)a.array[i] + (MAX_UINT32 + 1);
    tmp2 = (uint64_t)b.array[i] + borrow;
    res = (tmp1 - tmp2);
    rem->array[i] = (uint32_t)(res & MAX_UINT32);
    borrow = (res <= MAX_UINT32);
  }

  return borrow;
}

enum { GW_UINT256_SMALLER = -1, GW_UINT256_EQUAL = 0, GW_UINT256_LARGER = 1 };

int gw_uint256_cmp(const uint256_t a, const uint256_t b) {
  for (int i = 7; i >= 0; --i) {
    if (a.array[i] > b.array[i]) {
      return GW_UINT256_LARGER;
    } else if (a.array[i] < b.array[i]) {
      return GW_UINT256_SMALLER;
    }
  }

  return GW_UINT256_EQUAL;
}

#endif
