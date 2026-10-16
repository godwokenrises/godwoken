#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
export GW_V0_SETTLEMENT_PROFILE="$PWD/target/v0-settlement-test-profile.bin"
cargo +1.94.0 test --locked --features e2e-tests --test withdraw_e2e v0_settlement::export_v0_profile -- --nocapture
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_V0_SETTLEMENT_PROFILE.hash")
bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh
contract_target=$(cargo +1.94.0 metadata --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --format-version 1 --no-deps | jq -r '.target_directory')
export GW_V0_SETTLEMENT_BINARY="$PWD/target/v0-settlement-test-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/v0-settlement-state-validator" "$GW_V0_SETTLEMENT_BINARY"
"${LLVM_STRIP:-llvm-strip}" "$GW_V0_SETTLEMENT_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.94.0 test --locked --features e2e-tests --test withdraw_e2e v0_settlement -- --nocapture
