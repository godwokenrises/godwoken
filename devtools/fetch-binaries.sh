#!/bin/bash
dir_name='crates/builtin-binaries/builtin'
image='ghcr.io/godwokenrises/godwoken-prebuilds:dev-poly1.5.0'

docker pull $image
docker run --rm -v $(pwd)/$dir_name:/tmp $image bash -c "cp -r /scripts/* /tmp && echo 'Copy scripts'"
docker run --rm -v $(pwd)/$dir_name:/tmp $image bash -c "rm -rf /tmp/godwoken-polyjuice-v1.5.0 && mv /tmp/godwoken-polyjuice /tmp/godwoken-polyjuice-v1.5.0"
docker run --rm -v $(pwd)/$dir_name:/tmp $image bash -c "rm -rf /tmp/gwos-v1.3.0-rc1 && mv /tmp/godwoken-scripts /tmp/gwos-v1.3.0-rc1"
echo "Done"
