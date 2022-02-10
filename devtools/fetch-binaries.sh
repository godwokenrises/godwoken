#!/bin/bash
dir_name='.tmp/binaries'
docker pull ghcr.io/zeroqn/godwoken-prebuilds:develop-chore-rollup-config-allowed-type-hash-id
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-chore-rollup-config-allowed-type-hash-id cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-chore-rollup-config-allowed-type-hash-id cp -r /scripts/godwoken-polyjuice /tmp && echo "Copy polyjuice"
echo "Done"
