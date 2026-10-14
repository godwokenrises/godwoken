#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
export GW_WITHDRAW_TEST_PROFILE="$PWD/target/withdraw-test-profile.bin"
cargo +1.94.0 test --locked --features e2e-tests --test withdraw_e2e export_withdraw_profile -- --nocapture --skip settlement_e2e
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_WITHDRAW_TEST_PROFILE.hash")
bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh exit-withdraw-state-validator
contract_target=$(cargo +1.94.0 metadata --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --format-version 1 --no-deps | jq -r '.target_directory')
export GW_WITHDRAW_E2E_BINARY="$PWD/target/withdraw-test-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/exit-withdraw-state-validator" "$GW_WITHDRAW_E2E_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.94.0 test --locked --features e2e-tests -- --nocapture --skip settlement_e2e
