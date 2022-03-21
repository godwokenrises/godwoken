#!/bin/bash
dir_name='.tmp/binaries'
docker pull ghcr.io/flouse/godwoken-prebuilds:v1.0.x-202203101259
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/flouse/godwoken-prebuilds:v1.0.x-202203101259 cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
docker run --rm -v $(pwd)/$dir_name:/tmp ghcr.io/flouse/godwoken-prebuilds:v1.0.x-202203101259 cp -r /scripts/godwoken-polyjuice /tmp && echo "Copy polyjuice"
echo "Done"
