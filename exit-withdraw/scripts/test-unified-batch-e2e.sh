#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-batch}" in
  batch) export_test=export_batch_profile; test_name=twenty_account_batches ;;
  paged) export_test=export_paged_profile; test_name=paged_account_batch ;;
  *) echo 'Usage: test-unified-batch-e2e.sh [batch|paged]' >&2; exit 2 ;;
esac

mkdir -p target
export GW_V0_SETTLEMENT_PROFILE="$PWD/target/unified-${1:-batch}-profile.bin"
unset GW_V0_SETTLEMENT_BINARY
filter=v0_settlement::unified_batches
cargo +1.94.0 --config 'profile.dev.package.blake2b-ref.opt-level=3' test --locked --features e2e-tests --test withdraw_e2e \
  "$filter::$export_test" -- --ignored --nocapture --test-threads=1

contract_target=${GW_CONTRACT_TARGET_DIR:-$(cargo +1.94.0 metadata \
  --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml \
  --locked --format-version 1 --no-deps | jq -r '.target_directory')}
GW_EXIT_PROFILE_HASH=$(cat "$GW_V0_SETTLEMENT_PROFILE.hash") \
  CARGO_TARGET_DIR="$contract_target" \
  bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh v0-settlement-state-validator

export GW_V0_SETTLEMENT_BINARY="$PWD/target/unified-${1:-batch}-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/v0-settlement-state-validator" "$GW_V0_SETTLEMENT_BINARY"
if [ -n "${GW_LLVM_STRIP:-}" ]; then
  "$GW_LLVM_STRIP" "$GW_V0_SETTLEMENT_BINARY"
fi
cargo +1.94.0 --config 'profile.dev.package.blake2b-ref.opt-level=3' test --locked --features e2e-tests --test withdraw_e2e \
  "$filter::$test_name" -- --ignored --nocapture --test-threads=1
