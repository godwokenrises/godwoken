#ifndef GW_DLFCN_H_
#define GW_DLFCN_H_

#include "stdio.h"
/*
 * The ELF parsing code here is inspired from
 * https://github.com/riscv/riscv-pk/blob/master/pk/elf.h, original code is in
 * BSD license.
 */

typedef struct {
  uint8_t e_ident[16];
  uint16_t e_type;
  uint16_t e_machine;
  uint32_t e_version;
  uint64_t e_entry;
  uint64_t e_phoff;
  uint64_t e_shoff;
  uint32_t e_flags;
  uint16_t e_ehsize;
  uint16_t e_phentsize;
  uint16_t e_phnum;
  uint16_t e_shentsize;
  uint16_t e_shnum;
  uint16_t e_shstrndx;
} Elf64_Ehdr;

#define SHT_STRTAB 3
#define SHT_RELA 4
#define SHT_DYNSYM 11

typedef struct {
  uint32_t sh_name;
  uint32_t sh_type;
  uint64_t sh_flags;
  uint64_t sh_addr;
  uint64_t sh_offset;
  uint64_t sh_size;
  uint32_t sh_link;
  uint32_t sh_info;
  uint64_t sh_addralign;
  uint64_t sh_entsize;
} Elf64_Shdr;

#define PT_LOAD 1
#define PF_X 1

typedef struct {
  uint32_t p_type;
  uint32_t p_flags;
  uint64_t p_offset;
  uint64_t p_vaddr;
  uint64_t p_paddr;
  uint64_t p_filesz;
  uint64_t p_memsz;
  uint64_t p_align;
} Elf64_Phdr;

typedef struct {
  uint32_t st_name;
  uint8_t st_info;
  uint8_t st_other;
  uint16_t st_shndx;
  uint64_t st_value;
  uint64_t st_size;
} Elf64_Sym;

#define R_RISCV_RELATIVE 3

typedef struct {
  uint64_t r_offset;
  uint64_t r_info;
  int64_t r_addend;
} Elf64_Rela;

#define RISCV_PGSIZE 4096
#define ROUNDUP(a, b) ((((a)-1) / (b) + 1) * (b))
#define ROUNDDOWN(a, b) ((a) / (b) * (b))
#define MAX(a, b) ((a) > (b) ? (a) : (b))
#define MIN(a, b) ((a) < (b) ? (a) : (b))

#define ERROR_CONTEXT_FAILURE -21
#define ERROR_INVALID_ELF -22
#define ERROR_MEMORY_NOT_ENOUGH -23

typedef struct {
  Elf64_Sym *dynsyms;
  const char *dynstr;
  size_t dynsym_size;
  uint8_t *base_addr;
} CkbDlfcnContext;

int _sys_load_program_as_data(void *addr, uint64_t *len, size_t offset);

int _sys_load_program_as_code(void *addr, uint64_t memory_size,
                              uint64_t content_offset, uint64_t content_size);

int ckb_dlopen(uint8_t *aligned_addr, size_t aligned_size, void **handle,
               size_t *consumed_size) {
  if (sizeof(CkbDlfcnContext) > RISCV_PGSIZE || aligned_size < RISCV_PGSIZE) {
    return ERROR_CONTEXT_FAILURE;
  }

  CkbDlfcnContext *context = (CkbDlfcnContext *)aligned_addr;
  memset(context, 0, sizeof(CkbDlfcnContext));
  aligned_addr += RISCV_PGSIZE;
  aligned_size -= RISCV_PGSIZE;
  context->base_addr = aligned_addr;

  /* Basic ELF header parsing */
  Elf64_Ehdr header;
  uint64_t len = sizeof(header);
  int ret = _sys_load_program_as_data((void *)&header, &len, 0);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (len < sizeof(header)) {
    return ERROR_INVALID_ELF;
  }
  if ((header.e_phentsize != sizeof(Elf64_Phdr)) ||
      (header.e_shentsize != sizeof(Elf64_Shdr)) || (header.e_phnum > 16) ||
      (header.e_shnum > 32)) {
    return ERROR_INVALID_ELF;
  }
  /* Parse program headers and load relevant parts */
  Elf64_Phdr program_headers[16];
  len = sizeof(Elf64_Phdr) * header.e_phnum;
  ret =
      _sys_load_program_as_data((void *)program_headers, &len, header.e_phoff);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (len < sizeof(Elf64_Phdr) * header.e_phnum) {
    return ERROR_INVALID_ELF;
  }
  uint64_t max_consumed_size = 0;
  for (int i = 0; i < header.e_phnum; i++) {
    const Elf64_Phdr *ph = &program_headers[i];
    if (ph->p_type == PT_LOAD && ph->p_memsz > 0) {
      if ((ph->p_flags & PF_X) != 0) {
        uint64_t prepad = ph->p_vaddr % RISCV_PGSIZE;
        uint64_t vaddr = ph->p_vaddr - prepad;
        uint64_t memsz = ROUNDUP(prepad + ph->p_memsz, RISCV_PGSIZE);
        uint64_t size = 0;
        if (__builtin_uaddl_overflow(vaddr, memsz, &size)) {
          return ERROR_INVALID_ELF;
        }
        if (size > aligned_size) {
          return ERROR_MEMORY_NOT_ENOUGH;
        }
        ret = _sys_load_program_as_code(aligned_addr + vaddr, memsz,
                                        ph->p_offset, ph->p_filesz);
        if (ret != CKB_SUCCESS) {
          return ret;
        }
        max_consumed_size = MAX(max_consumed_size, vaddr + memsz);
      } else {
        uint64_t filesz = ph->p_filesz;
        uint64_t size = 0;
        if (__builtin_uaddl_overflow(ph->p_vaddr, filesz, &size)) {
          return ERROR_INVALID_ELF;
        }
        uint64_t consumed_end = ROUNDUP(size, RISCV_PGSIZE);
        if (consumed_end > aligned_size) {
          return ERROR_MEMORY_NOT_ENOUGH;
        }
        ret = _sys_load_program_as_data(aligned_addr + ph->p_vaddr, &filesz,
                                        ph->p_offset);
        if (ret != CKB_SUCCESS) {
          return ret;
        }
        if (filesz < ph->p_filesz) {
          return ERROR_INVALID_ELF;
        }
        max_consumed_size = MAX(max_consumed_size, consumed_end);
      }
    }
  }
  /*
   * Parse sectioin header & relocation headers,
   * Perform necessary relocations.
   */
  Elf64_Shdr section_headers[32];
  len = sizeof(Elf64_Shdr) * header.e_shnum;
  ret =
      _sys_load_program_as_data((void *)section_headers, &len, header.e_shoff);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (len < sizeof(Elf64_Shdr) * header.e_shnum) {
    return ERROR_INVALID_ELF;
  }
  /*
   * First load shstrtab tab, this is temporary code only needed in ELF loading
   * phase here.
   */
  Elf64_Shdr *shshrtab = &section_headers[header.e_shstrndx];
  char shrtab[4096];
  if (shshrtab->sh_size > 4096) {
    return ERROR_INVALID_ELF;
  }
  uint64_t shrtab_len = shshrtab->sh_size;
  ret = _sys_load_program_as_data((void *)shrtab, &shrtab_len,
                                  shshrtab->sh_offset);
  if (ret != CKB_SUCCESS) {
    return ret;
  }
  if (shrtab_len < shshrtab->sh_size) {
    return ERROR_INVALID_ELF;
  }
  for (int i = 0; i < header.e_shnum; i++) {
    const Elf64_Shdr *sh = &section_headers[i];
    if (sh->sh_type == SHT_RELA) {
      if (sh->sh_entsize != sizeof(Elf64_Rela)) {
        return ERROR_INVALID_ELF;
      }
      size_t relocation_size = sh->sh_size / sh->sh_entsize;
      uint64_t current_offset = sh->sh_offset;
      while (relocation_size > 0) {
        Elf64_Rela relocations[64];
        size_t load_size = MIN(relocation_size, 64);
        uint64_t load_length = load_size * sizeof(Elf64_Rela);
        ret = _sys_load_program_as_data((void *)relocations, &load_length,
                                        current_offset);
        if (ret != CKB_SUCCESS) {
          return ret;
        }
        if (len < load_size * sizeof(Elf64_Rela)) {
          return ERROR_INVALID_ELF;
        }
        relocation_size -= load_size;
        current_offset += len;
        for (int j = 0; j < load_size; j++) {
          Elf64_Rela *r = &relocations[j];
          if (r->r_info != R_RISCV_RELATIVE) {
            /* Only relative relocation is supported now, we might add more
             * later */
            return ERROR_INVALID_ELF;
          }
          *((uint64_t *)(aligned_addr + r->r_offset)) =
              (uint64_t)(aligned_addr + r->r_addend);
        }
      }
    } else if (sh->sh_type == SHT_DYNSYM) {
      /* We assume one ELF file only has one DYNSYM section now */
      if (sh->sh_entsize != sizeof(Elf64_Sym)) {
        return ERROR_INVALID_ELF;
      }
      context->dynsyms = (Elf64_Sym *)(aligned_addr + sh->sh_offset);
      context->dynsym_size = sh->sh_size / sh->sh_entsize;
    } else if (sh->sh_type == SHT_STRTAB) {
      const char *current_str = shrtab + sh->sh_name;
      if (strcmp(".dynstr", current_str) == 0) {
        context->dynstr = (const char *)(aligned_addr + sh->sh_offset);
      }
    }
  }
  if (context->dynsyms == NULL || context->dynstr == NULL) {
    return ERROR_INVALID_ELF;
  }
  *handle = (void *)context;
  *consumed_size = max_consumed_size + RISCV_PGSIZE;
  return CKB_SUCCESS;
}

void *ckb_dlsym(void *handle, const char *symbol) {
  CkbDlfcnContext *context = (CkbDlfcnContext *)handle;

  for (uint64_t i = 0; i < context->dynsym_size; i++) {
    Elf64_Sym *sym = &context->dynsyms[i];
    const char *str = context->dynstr + sym->st_name;
    if (strcmp(str, symbol) == 0) {
      return (void *)(context->base_addr + sym->st_value);
    }
  }

  return NULL;
}

#endif /* GW_DLFCN_H_ */