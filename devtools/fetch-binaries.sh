#!/bin/bash
dir_name='.tmp/binaries'
docker pull nervos/godwoken-prebuilds:latest
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp nervos/godwoken-prebuilds:latest cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
docker run --rm -v $(pwd)/$dir_name:/tmp nervos/godwoken-prebuilds:latest cp -r /scripts/godwoken-polyjuice /tmp && echo "Copy polyjuice"
# TODO: Wait prebuilds update
docker pull ghcr.io/zeroqn/godwoken-prebuilds:develop-feat--register-eth-eoa-mapping-on-deposit
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-feat--register-eth-eoa-mapping-on-deposit cp /scripts/godwoken-scripts/withdrawal-lock /tmp/godwoken-scripts/withdrawal-lock && echo "Override withdrawal-lock"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-feat--register-eth-eoa-mapping-on-deposit cp /scripts/godwoken-scripts/state-validator /tmp/godwoken-scripts/state-validator && echo "Override state-validator"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/zeroqn/godwoken-prebuilds:develop-feat--register-eth-eoa-mapping-on-deposit cp -r /scripts/godwoken-polyjuice /tmp && echo "override polyjuice"
echo "Done"
