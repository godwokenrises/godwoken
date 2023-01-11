# gw-builtin-binaries

This crate contains builtin binaries from `/gwos` and `/gwos-evm`.

## Usage

The `build.rs` file contains checksum and path of binaries, edit the file to add new binaries or update the bundled file path.

To generate `builtin/checksum.txt`

``` bash
cd builtin
find . -not -path checksum.txt -type f -exec sha256sum {} \;
```

## Release binaries

The builtin binaries is stored in the `crates/builtin-binaries/builtin` submodule.

To add new binaries:

1. Update local submodule to the head `cd crates/builtin-binaries/builtin && git pull`
2. Edit `crate/builtin-binaries/build.rs` to copy new binaries from build directory to submodule path
3. Create new PR, the GitHub action `prepare-builtin-binaries.yml` will push binaries to submodule repo once the PR is merged.
