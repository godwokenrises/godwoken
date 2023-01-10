#!/bin/bash
dir_name='crates/builtin-binaries/builtin'
image='ghcr.io/godwokenrises/godwoken-prebuilds:dev-poly1.5.0'

docker pull $image
[ -d $dir_name ] && rm -rf $dir_name && echo "Delete old dir"
mkdir -p $dir_name && echo "Create dir"
docker run --rm -v $(pwd)/$dir_name:/tmp $image bash -c "cp -r /scripts/* /tmp && echo 'Copy scripts'"
mv $dir_name/godwoken-polyjuice $dir_name/godwoken-polyjuice-v1.5.0
mkdir $dir_name/godwoken-polyjuice
[ -d gwos-evm/build ] && cp gwos-evm/build/generator $dir_name/godwoken-polyjuice/ && echo 'Copy current polyjuice build'
echo "Done"
