#!/bin/bash
dir_name='.tmp/binaries'
docker pull ghcr.io/zeroqn/godwoken-prebuilds:develop-fix-secp256k1-witness-limit
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-fix-secp256k1-witness-limit cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-fix-secp256k1-witness-limit cp -r /scripts/godwoken-polyjuice /tmp && echo "Copy polyjuice"
echo "Done"
