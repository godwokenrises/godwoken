#!/bin/bash
dir_name='.tmp/binaries'
GODWOKEN_TAG=v0.10.3
docker pull nervos/godwoken-prebuilds:$GODWOKEN_TAG
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp nervos/godwoken-prebuilds:$GODWOKEN_TAG cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
# TODO: Wait prebuilds update
docker pull ghcr.io/zeroqn/godwoken-prebuilds:dev-feat-fast-withdrawal-to-v1
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:dev-feat-fast-withdrawal-to-v1 cp /scripts/godwoken-scripts/withdrawal-lock /tmp/godwoken-scripts/withdrawal-lock && echo "Override withdrawal-lock"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:dev-feat-fast-withdrawal-to-v1 cp /scripts/godwoken-scripts/state-validator /tmp/godwoken-scripts/state-validator && echo "Override state-validator"
echo "Done"

