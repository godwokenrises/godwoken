// # State Lock
//
// Lock script used for PoA Setup cell and PoA Data cell.
//
// It only tests if current transaction has a cell with matching lock.
// During initialization phase, the main PoA cell, setup cell and data cell
// should be created together. The detailed steps are:
//
// * PoA cell is set to PoA lock implemented in poa.c
// * PoA Setup cell and PoA Data cell are set to State Lock implemented in
// state.c
// * Generate type ID 1 for PoA Setup cell
// * Generate type ID 2 for PoA Data cell
// * Use type ID 1 and 2 to fill in PoA lock in the main PoA cell
// * Calculate PoA lock hash, and use the lock hash to fill in args part for the
// locks of PoA Setup cell and PoA Data cell
//
// Since the 3 cells are created in a single transaction, this flow works.
#include "blockchain.h"
#include "ckb_syscalls.h"

#define SCRIPT_BUFFER_SIZE 128

#ifdef ENABLE_DEBUG_MODE
#define DEBUG(s) ckb_debug(s)
#else
#define DEBUG(s)
#endif /* ENABLE_DEBUG_MODE */

#define ERROR_TRANSACTION -1

int main() {
  // Load current script so as to extract PoA cell information
  uint8_t script[SCRIPT_BUFFER_SIZE];
  uint64_t len = SCRIPT_BUFFER_SIZE;
  int ret = ckb_checked_load_script(script, &len, 0);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  mol_seg_t script_seg;
  script_seg.ptr = (uint8_t *)script;
  script_seg.size = len;
  if (MolReader_Script_verify(&script_seg, false) != MOL_OK) {
    DEBUG("molecule verification failure!");
    return ERROR_TRANSACTION;
  }
  mol_seg_t args_seg = MolReader_Script_get_args(&script_seg);
  mol_seg_t args_bytes_seg = MolReader_Bytes_raw_bytes(&args_seg);

  if (args_bytes_seg.size != 32) {
    DEBUG("Script args must be 32 bytes long!");
    return ERROR_TRANSACTION;
  }

  size_t current = 0;
  while (current < SIZE_MAX) {
    uint8_t hash[32];
    len = 32;

    ret = ckb_load_cell_by_field(hash, &len, 0, current, CKB_SOURCE_INPUT,
                                 CKB_CELL_FIELD_LOCK_HASH);
    if (ret != CKB_SUCCESS) {
      return ret;
    }
    if (len != 32) {
      DEBUG("Invalid script length!");
      return ERROR_TRANSACTION;
    }
    if (memcmp(hash, args_bytes_seg.ptr, 32) == 0) {
      break;
    }
    current++;
  }
  return 0;
}
