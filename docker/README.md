godwoken-docker-prebuilds
=========================

Docker image containing all binaries used by Godwoken, saving you the hassles of building them yourself.

How to build:

```bash
$ make build-components
$ docker build . -t godwoken-prebuilds
```

How to upgrade:

```bash
$ # make sure it pass test..
$ make test
$ # build and push to docker-hub, will ask you to enter image tag
$ make build-push
```

## Usage

`Godwoken` binary resides in `/bin/godwoken`, this is already in PATH so you can do this:

```bash
$ docker run --rm godwoken-prebuilds godwoken --version
```

`gw-tools` can be used in the same way:

```bash
$ docker run --rm godwoken-prebuilds gw-tools --version
```

CKB and ckb-cli are also available this way:

```bash
$ docker run --rm godwoken-prebuilds ckb --version
$ docker run --rm godwoken-prebuilds ckb-cli --version
```

## CPU Feature Requirement

Starting from version 1.3.0-rc3, the published images require [AVX2](https://en.wikipedia.org/wiki/Advanced_Vector_Extensions). Most recent x86-64 CPUs support AVX2. On linux, you can check for AVX2 support by inspecting `/proc/cpuinfo` or running `lscpu`.

If your CPU/environment does not support AVX2, you can build Godwoken targeting your specific CPU/environment:
```sh
cd build/godwoken && rustup component add rustfmt && RUSTFLAGS="-C target-cpu=native" CARGO_PROFILE_RELEASE_LTO=true cargo build --release
```

## Check the reference of every component
```bash
docker inspect godwoken-prebuilds:[TAG] | egrep ref.component
```

## Scripts

All the scripts used by Godwoken can be found at `/scripts` folder:

```bash
$ docker run --rm godwoken-prebuilds find /scripts -type f -exec sha1sum {} \;
```

### Result
refer to [checksum.txt](./checksum.txt)
