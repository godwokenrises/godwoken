#!/bin/bash
dir_name='.tmp/binaries'
docker pull nervos/godwoken-prebuilds:latest
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run -v $(pwd)/$dir_name:/tmp nervos/godwoken-prebuilds:latest cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
echo "Done"