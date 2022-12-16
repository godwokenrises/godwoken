#!/usr/bin/env bash

# Query whether web3 api is ready to serve
echo '{
  "id": 42,
  "jsonrpc": "2.0",
  "method": "poly_getHealthStatus",
  "params": []
}' \
| tr -d '\n' \
| curl --silent -H 'content-type: application/json' -d @- \
http://127.0.0.1:8024 \
| jq '.result.status' | egrep "true" || exit 1
