#!/usr/bin/env bash

set -e
set -u
[ -n "${DEBUG:-}" ] && set -x || true

main() {
  if [ $# != 1 ]; then
    echo "bump.sh version" >&2
    exit 1
  fi
  local v="$1"

  # Update the version in Cargo.toml files
  find ./crates ./gwos/crates ./web3 -name 'Cargo.toml' ! -regex './crates/autorocks/.*' -print0 | xargs -0 sed -i.bak \
    -e 's/^version = .*/version = "'"$v"'"/' \
    -e '/autorocks/! s/\({.*path = ".*",.* version = "= \)[^"]*/\1'"$v"'/'
  find . -name 'Cargo.toml.bak' -exec rm -f {} \;
  cargo check
  cd web3 && cargo check && cd ..

  # Update the version in package.json files
  find ./web3 -name 'package.json' -print0 | xargs -0 sed -i.bak \
    -e 's/^\([[:blank:]]*\)"version": .*,/\1"version": "'"$v"'",/' \
    -e 's|^\([[:blank:]]*\)"@godwoken-web3/godwoken": .*,|\1"@godwoken-web3/godwoken": "'"$v"'",|'
  find . -name 'package.json.bak' -exec rm -f {} \;
  cd web3 && yarn upgrade -P godwoken-web3 --frozen-lockfile && cd ..
}

main "$@"
