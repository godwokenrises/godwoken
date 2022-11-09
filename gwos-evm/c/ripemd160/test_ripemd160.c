
#include "ripemd160.h"
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

void hex2bin(const char *hex, uint8_t **out, size_t *out_len) {
  size_t hex_length = strlen(hex);
  *out_len = hex_length / 2;
  *out = (uint8_t *)malloc(*out_len);
  char part1;
  char part2;
  for (size_t i = 0; i < *out_len; i++) {
    part1 = hex[i*2];
    part2 = hex[i*2 + 1];
    part1 = part1 >= 'a' ? (part1 - 'a' + 10) : (part1 - '0');
    part2 = part2 >= 'a' ? (part2 - 'a' + 10) : (part2 - '0');
    (*out)[i] = (uint8_t)(part1 * 16 + part2);
  }
}

static char data_buffer[64 * 1024];
void print_hex(const char* prefix, const uint8_t *data, size_t data_len) {
  int offset = 0;
  offset += sprintf(data_buffer, "%s 0x", prefix);
  for (size_t i = 0; i < data_len; i++) {
    offset += sprintf(data_buffer + offset, "%02x", data[i]);
  }
  data_buffer[offset] = '\0';
  printf("%s\n", data_buffer);
}

int test_case(const char *title, const char *msg_str, const char *expected_hash_hex) {
  uint8_t *expected_hash = NULL;
  size_t expected_hash_size = 0;
  hex2bin(expected_hash_hex, &expected_hash, &expected_hash_size);
  if (expected_hash_size != RIPEMD160_DIGEST_LENGTH) {
    printf("invalid expected hash size: %ld\n", (uint64_t)expected_hash_size);
    return -1;
  }

  printf("[msg] %s\n", msg_str);
  uint8_t *msg = (uint8_t *)msg_str;
  uint32_t msg_size = (uint32_t)strlen(msg_str);

  uint8_t hash[RIPEMD160_DIGEST_LENGTH] = {0};
  ripemd160(msg, msg_size, hash);
  print_hex("[hash]", hash, RIPEMD160_DIGEST_LENGTH);
  if (memcmp(hash, expected_hash, RIPEMD160_DIGEST_LENGTH) != 0) {
    printf("invalid expected hash: %s => %s\n", msg_str, expected_hash_hex);
    return -2;
  }
  printf("test <%s> ok\n\n", title);
  return 0;
}

int main() {
  if (test_case("ripemd160 Test vector from paper #1", "", "9c1185a5c5e9fc54612808977ee8f548b2258d31") != 0) {
    return -1;
  }
  if (test_case("ripemd160 Test vector from paper #2", "a", "0bdc9d2d256b3ee9daae347be6f4dc835a467ffe") != 0) {
    return -2;
  }
  if (test_case("ripemd160 Test vector from paper #3", "abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc") != 0) {
    return -3;
  }
  if (test_case("ripemd160 Test vector from paper #4", "message digest", "5d0689ef49d2fae572b881b123a85ffa21595f36") != 0) {
    return -4;
  }
  if (test_case("ripemd160 Test vector from paper #5", "abcdefghijklmnopqrstuvwxyz", "f71c27109c692c1b56bbdceb5b9d2865b3708dbc") != 0) {
    return -5;
  }
  if (test_case("ripemd160 Test vector from paper #6", "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", "12a053384a9c0c88e405a06c27dcf49ada62eb2b") != 0) {
    return -6;
  }
  if (test_case("ripemd160 Test vector from paper #7", "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", "b0e20b6e3116640286ed3a87a5713079b21f5189") != 0) {
    return -7;
  }
  if (test_case("ripemd160 Test vector from paper #8", "12345678901234567890123456789012345678901234567890123456789012345678901234567890", "9b752e45573d4b39f4dbd3323cab82bf63326bfb") != 0) {
    return -8;
  }
}
