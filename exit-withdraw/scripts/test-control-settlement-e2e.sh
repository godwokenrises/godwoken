#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export GW_CONTROL_TEST=1
export GW_CONTROL_BROWSER_FIXTURE="${GW_CONTROL_BROWSER_FIXTURE:-$PWD/claim-web/tests/control-v1-fixture.json}"
export GW_CONTROL_BROWSER_FIXTURE_V0="${GW_CONTROL_BROWSER_FIXTURE_V0:-$PWD/claim-web/tests/control-v0-fixture.json}"
mkdir -p target
export GW_SETTLEMENT_TEST_PROFILE="$PWD/target/control-v1-profile.bin"
cargo +1.95.0 test --locked --offline --features e2e-tests --test withdraw_e2e settlement_e2e::export_settlement_profile -- --nocapture
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_SETTLEMENT_TEST_PROFILE.hash")
bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh
contract_target=$(cargo +1.95.0 metadata --manifest-path ../gwos/contracts/exit-withdraw-state-validator/Cargo.toml --locked --format-version 1 --no-deps | jq -r '.target_directory')
export GW_WITHDRAW_E2E_BINARY="$PWD/target/control-v1-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/settlement-state-validator" "$GW_WITHDRAW_E2E_BINARY"
"${LLVM_STRIP:-llvm-strip}" "$GW_WITHDRAW_E2E_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.95.0 test --locked --offline --features e2e-tests --test withdraw_e2e control_v1_full -- --nocapture
unset GW_WITHDRAW_E2E_BINARY
export GW_V0_SETTLEMENT_PROFILE="$PWD/target/control-v0-profile.bin"
cargo +1.95.0 test --locked --offline --features e2e-tests --test withdraw_e2e v0_settlement::export_v0_profile -- --nocapture
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_V0_SETTLEMENT_PROFILE.hash")
bash ../gwos/contracts/exit-withdraw-state-validator/scripts/build.sh
export GW_V0_SETTLEMENT_BINARY="$PWD/target/control-v0-validator"
cp "$contract_target/riscv64imac-unknown-none-elf/release/v0-settlement-state-validator" "$GW_V0_SETTLEMENT_BINARY"
"${LLVM_STRIP:-llvm-strip}" "$GW_V0_SETTLEMENT_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.95.0 test --locked --offline --features e2e-tests --test withdraw_e2e control_v0_full -- --nocapture
