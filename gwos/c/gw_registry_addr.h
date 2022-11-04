/*
 * Godwoken registry address format
 *
 * ## Terms
 *
 * Registry:
 * Registry is a concept in the Godwoken, registry mapping Godwoken script hash
 * to native addresses. Such as Ethereum address. Registry itself is a Godwoken
 * contract.
 *
 * ## Storage format
 *
 * `registry_id(4 bytes) | address len (4 bytes) | address(n bytes)`
 */

#ifndef GW_REGISTRY_H_
#define GW_REGISTRY_H_

#include "gw_errors.h"

/* macros */
#define GW_REG_ADDR_SIZE(addr) (8 + addr.addr_len)

typedef struct gw_reg_addr {
  uint32_t reg_id;
  uint32_t addr_len;
  /* for simplify, we use a constant length 32. In theory this address is
   * unlimited */
  uint8_t addr[32];
} gw_reg_addr_t;

/* the buf must be greater than addr */
void _gw_cpy_addr(uint8_t *buf, gw_reg_addr_t addr) {
  memcpy(buf, (uint8_t *)(&addr.reg_id), 4);
  memcpy(buf + 4, (uint8_t *)(&addr.addr_len), 4);
  memcpy(buf + 8, addr.addr, addr.addr_len);
}

/* parse addr */
int _gw_parse_addr(uint8_t *buf, int len, gw_reg_addr_t *addr) {
  if (len < 8) {
    return GW_FATAL_INVALID_DATA;
  }
  memcpy((uint8_t *)&addr->reg_id, buf, 4);
  memcpy((uint8_t *)(&addr->addr_len), buf + 4, 4);
  /* Only support addr_len <=20 for now */
  if (addr->addr_len > 20) {
    printf("failed to parse addr, addr len is large than 20");
    return GW_FATAL_BUFFER_OVERFLOW;
  }
  if ((int)(addr->addr_len + 8) > len) {
    return GW_FATAL_INVALID_DATA;
  }
  memcpy(addr->addr, buf + 8, addr->addr_len);
  return 0;
}

/* return 0 if addr is same, otherwise return non zero */
int _gw_cmp_addr(gw_reg_addr_t addr_a, gw_reg_addr_t addr_b) {
  if (addr_a.reg_id != addr_b.reg_id) {
    return -1;
  }
  if (addr_a.addr_len != addr_b.addr_len) {
    return -1;
  }
  return memcmp(addr_a.addr, addr_b.addr, addr_a.addr_len);
}

#endif
