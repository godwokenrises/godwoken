#include "common.h"

#define _GW_SMT_STACK_SIZE 32

/* Key Value Pairs */
typedef struct {
  uint8_t key[GW_KEY_BYTES];
  uint8_t value[GW_VALUE_BYTES];
  uint8_t order;
} gw_pair_t;

typedef struct {
  gw_pair_t *pairs;
  uint32_t len;
  uint32_t capacity;
} gw_state_t;

void gw_state_init(gw_state_t *state, gw_pair_t *buffer, uint32_t capacity) {
  state->pairs = buffer;
  state->len = 0;
  state->capacity = capacity;
}

int gw_state_insert(gw_state_t *state, const uint8_t key[GW_KEY_BYTES],
                    const uint8_t value[GW_VALUE_BYTES]) {
  if (state->len < state->capacity) {
    /* shortcut, append at end */
    memcpy(state->pairs[state->len].key, key, GW_KEY_BYTES);
    memcpy(state->pairs[state->len].value, value, GW_KEY_BYTES);
    state->len++;
    return 0;
  }

  /* Find a matched key and overwritten it */
  int32_t i = state->len - 1;
  for (; i >= 0; i--) {
    if (memcmp(key, state->pairs[i].key, GW_KEY_BYTES) == 0) {
      break;
    }
  }

  if (i < 0) {
    return GW_ERROR_INSUFFICIENT_CAPACITY;
  }

  memcpy(state->pairs[i].value, value, GW_VALUE_BYTES);
  return 0;
}

int gw_state_fetch(gw_state_t *state, const uint8_t key[GW_KEY_BYTES],
                   uint8_t value[GW_VALUE_BYTES]) {
  int32_t i = state->len - 1;
  for (; i >= 0; i--) {
    if (memcmp(key, state->pairs[i].key, GW_KEY_BYTES) == 0) {
      memcpy(value, state->pairs[i].value, GW_VALUE_BYTES);
      return 0;
    }
  }
  return GW_ERROR_NOT_FOUND;
}

int _gw_pair_cmp(const void *a, const void *b) {
  const gw_pair_t *pa = (const gw_pair_t *)a;
  const gw_pair_t *pb = (const gw_pair_t *)b;

  for (uint32_t i = GW_KEY_BYTES - 1; i >= 0; i--) {
    int cmp_result = pa->key[i] - pb->key[i];
    if (cmp_result != 0) {
      return cmp_result;
    }
  }
  return pa->order - pb->order;
}

void gw_state_normalize(gw_state_t *state) {
  for (uint32_t i = 0; i < state->len; i++) {
    state->pairs[i].order = i;
  }
  qsort(state->pairs, state->len, sizeof(gw_pair_t), _gw_pair_cmp);
  /* Remove duplicate ones */
  int32_t sorted = 0, next = 0;
  while (next < (int32_t)state->len) {
    int32_t item_index = next++;
    while (next < (int32_t)state->len &&
           memcmp(state->pairs[item_index].key, state->pairs[next].key,
                  GW_KEY_BYTES) == 0) {
      next++;
    }
    if (item_index != sorted) {
      memcpy(state->pairs[sorted].key, state->pairs[item_index].key,
             GW_KEY_BYTES);
      memcpy(state->pairs[sorted].value, state->pairs[item_index].value,
             GW_VALUE_BYTES);
    }
    sorted++;
  }
  state->len = sorted;
}

/* SMT */

int _gw_get_bit(const uint8_t *data, int offset) {
  int byte_pos = offset / 8;
  int bit_pos = offset % 8;
  return ((data[byte_pos] >> bit_pos) & 1) != 0;
}

void _gw_set_bit(uint8_t *data, int offset) {
  int byte_pos = offset / 8;
  int bit_pos = offset % 8;
  data[byte_pos] |= 1 << bit_pos;
}

void _gw_clear_bit(uint8_t *data, int offset) {
  int byte_pos = offset / 8;
  int bit_pos = offset % 8;
  data[byte_pos] &= (uint8_t)(~(1 << bit_pos));
}

void _gw_copy_bits(uint8_t source[32], int first_kept_bit) {
  int first_byte = first_kept_bit / 8;
  for (int i = 0; i < first_byte; i++) {
    source[i] = 0;
  }
  for (int i = first_byte * 8; i < first_kept_bit; i++) {
    _gw_clear_bit(source, i);
  }
}

void _gw_parent_path(uint8_t key[32], uint8_t height) {
  if (height == 255) {
    memset(key, 0, 32);
  } else {
    _gw_copy_bits(key, height + 1);
  }
}

int gw_smt_calculate_root(uint8_t buffer[32], const gw_state_t *state,
                          const uint8_t *proof, uint32_t proof_length) {
  blake2b_state blake2b_ctx;
  uint8_t stack_keys[_GW_SMT_STACK_SIZE][GW_KEY_BYTES];
  uint8_t stack_values[_GW_SMT_STACK_SIZE][32];
  uint32_t proof_index = 0;
  uint32_t leave_index = 0;
  uint32_t stack_top = 0;

  while (proof_index < proof_length) {
    switch (proof[proof_index++]) {
      case 0x4C:
        if (stack_top >= _GW_SMT_STACK_SIZE) {
          return GW_ERROR_INVALID_STACK;
        }
        if (leave_index >= state->len) {
          return GW_ERROR_INVALID_PROOF;
        }
        memcpy(stack_keys[stack_top], state->pairs[leave_index].key,
               GW_KEY_BYTES);
        blake2b_init(&blake2b_ctx, 32);
        blake2b_update(&blake2b_ctx, state->pairs[leave_index].key,
                       GW_KEY_BYTES);
        blake2b_update(&blake2b_ctx, state->pairs[leave_index].value,
                       GW_KEY_BYTES);
        blake2b_final(&blake2b_ctx, stack_values[stack_top], 32);
        stack_top++;
        leave_index++;
        break;
      case 0x50: {
        if (stack_top == 0) {
          return GW_ERROR_INVALID_STACK;
        }
        if (proof_index + 33 > proof_length) {
          return GW_ERROR_INVALID_PROOF;
        }
        uint8_t height = proof[proof_index++];
        const uint8_t *current_proof = &proof[proof_index];
        proof_index += 32;
        uint8_t *key = stack_keys[stack_top - 1];
        uint8_t *value = stack_values[stack_top - 1];
        blake2b_init(&blake2b_ctx, 32);
        if (_gw_get_bit(key, height)) {
          blake2b_update(&blake2b_ctx, current_proof, 32);
          blake2b_update(&blake2b_ctx, value, 32);
        } else {
          blake2b_update(&blake2b_ctx, value, 32);
          blake2b_update(&blake2b_ctx, current_proof, 32);
        }
        blake2b_final(&blake2b_ctx, value, 32);
        _gw_parent_path(key, height);
      } break;
      case 0x48: {
        if (stack_top < 2) {
          return GW_ERROR_INVALID_STACK;
        }
        if (proof_index >= proof_length) {
          return GW_ERROR_INVALID_PROOF;
        }
        uint8_t height = proof[proof_index++];
        uint8_t *key_a = stack_keys[stack_top - 2];
        uint8_t *value_a = stack_values[stack_top - 2];
        uint8_t *key_b = stack_keys[stack_top - 1];
        uint8_t *value_b = stack_values[stack_top - 1];
        stack_top -= 2;
        int a_set = _gw_get_bit(key_a, height);
        int b_set = _gw_get_bit(key_b, height);
        _gw_copy_bits(key_a, height);
        _gw_copy_bits(key_b, height);
        uint8_t sibling_key_a[32];
        memcpy(sibling_key_a, key_a, 32);
        if (!a_set) {
          _gw_set_bit(sibling_key_a, height);
        }
        if (memcmp(sibling_key_a, key_b, 32) != 0 || (a_set == b_set)) {
          return GW_ERROR_INVALID_SIBLING;
        }
        blake2b_init(&blake2b_ctx, 32);
        if (a_set) {
          blake2b_update(&blake2b_ctx, value_b, 32);
          blake2b_update(&blake2b_ctx, value_a, 32);
        } else {
          blake2b_update(&blake2b_ctx, value_a, 32);
          blake2b_update(&blake2b_ctx, value_b, 32);
        }
        /* Top-of-stack key is already updated to parent_key_a */
        blake2b_final(&blake2b_ctx, value_a, 32);
        stack_top++;
      } break;
      default:
        return GW_ERROR_INVALID_PROOF;
    }
  }
  /* All leaves must be used */
  if (leave_index != state->len) {
    return GW_ERROR_INVALID_PROOF;
  }
  if (stack_top != 1) {
    return GW_ERROR_INVALID_STACK;
  }
  memcpy(buffer, stack_values[0], 32);
  return 0;
}

int gw_smt_verify(const uint8_t hash[32], const gw_state_t *state,
                  const uint8_t *proof, uint32_t proof_length) {
  uint8_t buffer[32];
  int ret = gw_smt_calculate_root(buffer, state, proof, proof_length);
  if (ret != 0) {
    return ret;
  }
  if (memcmp(buffer, hash, 32) != 0) {
    return GW_ERROR_INVALID_PROOF;
  }
  return 0;
}
