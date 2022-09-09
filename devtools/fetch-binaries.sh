#!/bin/bash
dir_name='.tmp/binaries'
image='ghcr.io/zeroqn/godwoken-prebuilds:v1.6-feat-remove-withdrawal-cell'

docker pull $image
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp $image cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
docker run --rm -v $(pwd)/$dir_name:/tmp $image cp -r /scripts/godwoken-polyjuice /tmp && echo "Copy polyjuice"
echo "Done"
