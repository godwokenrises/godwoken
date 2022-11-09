#!/usr/bin/env bash

set -e
set -u
[ -n "${DEBUG:-}" ] && set -x || true

SCRIPT_DIR=$(realpath $(dirname $0))
PROJECT_ROOT=$(dirname $SCRIPT_DIR)

main() {
  if [ $# != 1 ]; then
    echo "bump.sh version" >&2
    exit 1
  fi

  local v="$1"
  sed -i 's|POLYJUICE_VERSION "v.*"|POLYJUICE_VERSION "v'"$v"'"|' $PROJECT_ROOT/c/polyjuice_globals.h
  sed -i 's/^version = .*/version = "'"$v"'"/' $PROJECT_ROOT/polyjuice-tests/Cargo.toml

  cd polyjuice-tests && cargo check
}

main "$@"
