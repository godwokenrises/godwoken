#!/usr/bin/env bash
# Build only the selected release artifact, pinned to the decoded binary profile.
set -euo pipefail
if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  echo 'Usage: GW_WITHDRAW_CLI=/path/to/gw-withdraw build-settlement-validator.sh PROFILE_DIRECTORY OUTPUT_BINARY [v1|v0]' >&2
  exit 2
fi
profile_dir=$(cd "$1" && pwd)
mkdir -p "$(dirname "$2")"
output="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
test ! -e "$output"
test ! -e "$output.json"
cli=${GW_WITHDRAW_CLI:?Set GW_WITHDRAW_CLI to the built Rust executable}
protocol=${3:-v1}
case "$protocol" in
  v1) binary=settlement-state-validator ;;
  v0) binary=v0-settlement-state-validator ;;
  *) echo 'Protocol must be v1 or v0' >&2; exit 2 ;;
esac
profile_info=$("$cli" settlement --protocol "$protocol" inspect-profile --profile "$profile_dir/profile.bin")
export GW_EXIT_PROFILE_HASH
GW_EXIT_PROFILE_HASH=$(jq -r '.profile_hash' <<<"$profile_info")
GW_EXIT_PROFILE_HASH=${GW_EXIT_PROFILE_HASH#0x}
cd "$(dirname "$0")/../../gwos/contracts/exit-withdraw-state-validator"
bash scripts/build.sh "$binary"
target_dir=$(cargo +1.94.0 metadata --locked --format-version 1 --no-deps | jq -r '.target_directory')
# Strip symbols to avoid locking unnecessary CKB in executable data.
strip_tool=${LLVM_STRIP:-llvm-strip}
command -v "$strip_tool" >/dev/null
"$strip_tool" -s -o "$output" "$target_dir/riscv64imac-unknown-none-elf/release/$binary"
if command -v sha256sum >/dev/null; then
  binary_sha=$(sha256sum "$output" | awk '{print $1}')
else
  binary_sha=$(shasum -a 256 "$output" | awk '{print $1}')
fi
jq -n --argjson profile "$profile_info" --arg hash "$binary_sha" --arg binary "$binary" '{binary:$binary,profile:$profile,validator_sha256:$hash,deployed:false}' > "$output.json"
