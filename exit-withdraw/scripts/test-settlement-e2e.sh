#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
export GW_SETTLEMENT_TEST_PROFILE="$PWD/target/settlement-test-profile.bin"
cargo +1.95.0 test --locked --features e2e-tests --test withdraw_e2e settlement_e2e::export_settlement_profile -- --nocapture
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_SETTLEMENT_TEST_PROFILE.hash")
bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh
contract_target=$(cargo +1.95.0 metadata --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --format-version 1 --no-deps | jq -r '.target_directory')
export GW_WITHDRAW_E2E_BINARY="$PWD/target/settlement-test-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/settlement-state-validator" "$GW_WITHDRAW_E2E_BINARY"
"${LLVM_STRIP:-llvm-strip}" "$GW_WITHDRAW_E2E_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.95.0 test --locked --features e2e-tests --test withdraw_e2e settlement_e2e -- --nocapture
