#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
export GW_EXIT_TEST_PROFILE="$PWD/target/test-profile.bin"
cargo +1.94.0 test --locked --features vm-tests --test vm export_test_profile -- --nocapture
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(cat "$GW_EXIT_TEST_PROFILE.hash")
bash scripts/build.sh exit-withdraw-state-validator
# Copy the test binary under an explicit fixture-only name; never use it for deployment.
target_dir=$(cargo +1.94.0 metadata --locked --format-version 1 --no-deps | jq -r '.target_directory')
export GW_EXIT_VM_BINARY="$PWD/target/test-validator"
cp "$target_dir/riscv64imac-unknown-none-elf/release/exit-withdraw-state-validator" "$GW_EXIT_VM_BINARY"
unset GW_EXIT_PROFILE_HASH
cargo +1.94.0 test --locked --features vm-tests -- --nocapture
