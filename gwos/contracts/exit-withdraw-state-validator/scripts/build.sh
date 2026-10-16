#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Build only the V3 settlement validators for the existing V0/V1 networks.
if [ "$#" -gt 1 ]; then
  echo 'Usage: build.sh [settlement-state-validator|v0-settlement-state-validator]' >&2
  exit 2
fi
features=onchain
case "${1:-settlement}" in
  settlement) binaries=(--bin settlement-state-validator --bin v0-settlement-state-validator) ;;
  settlement-state-validator|v0-settlement-state-validator) binaries=(--bin "$1") ;;
  *) echo 'Unknown validator binary' >&2; exit 2 ;;
esac
# Prerequisites: Rust 1.94.0, riscv64imac target, clang, llvm-ar, jq.
# A missing profile deliberately produces a fail-closed binary.
export CC_riscv64imac_unknown_none_elf="${CC_riscv64imac_unknown_none_elf:-clang}"
export AR_riscv64imac_unknown_none_elf="${AR_riscv64imac_unknown_none_elf:-llvm-ar}"
stdlib_manifest=$(cargo +1.94.0 metadata --locked --format-version 1 --features onchain --filter-platform riscv64imac-unknown-none-elf | jq -r '.packages[] | select(.name == "ckb-std") | .manifest_path')
stdlib_dir=$(dirname "$stdlib_manifest")/c/ckb-c-stdlib/libc
export CFLAGS_riscv64imac_unknown_none_elf="--target=riscv64 -march=rv64imc -mabi=lp64 -DCKB_DECLARATION_ONLY -I${stdlib_dir}"
# CKB-VM has no RISC-V A extension. ckb-std supplies single-threaded shims.
export CARGO_TARGET_RISCV64IMAC_UNKNOWN_NONE_ELF_RUSTFLAGS='-C target-feature=-a'
cargo +1.94.0 build --locked --release --features "$features" "${binaries[@]}" --target riscv64imac-unknown-none-elf
