
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void hex2bin(const char *hex, uint8_t **out, size_t *out_len) {
  size_t hex_length = strlen(hex);
  *out_len = hex_length / 2;
  *out = (uint8_t *)malloc(*out_len);
  char part1;
  char part2;
  for (size_t i = 0; i < *out_len; i++) {
    part1 = hex[i*2];
    part2 = hex[i*2 + 1];
    part1 = part1 >= 'a' ? (part1 - 'a' + 10) : (part1 >= 'A' ? (part1 - 'A' + 10) : (part1 - '0'));
    part2 = part2 >= 'a' ? (part2 - 'a' + 10) : (part2 >= 'A' ? (part2 - 'A' + 10) : (part2 - '0'));
    (*out)[i] = (uint8_t)(part1 * 16 + part2);
  }
}
