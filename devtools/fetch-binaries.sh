#!/bin/bash
dir_name='.tmp/binaries'
IMAGE=ghcr.io/nervosnetwork/godwoken-prebuilds:v0.10.7
docker pull $IMAGE
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp $IMAGE cp -r /scripts/godwoken-scripts /tmp && echo "Copy scripts"
echo "Done"
