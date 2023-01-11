# gw-builtin-binaries

This crate contains builtin binaries from `/gwos` and `/gwos-evm`.

## Usage

The `build.rs` file contains checksum and path of binaries, edit the file to add new binaries or update the bundled file path.

To generate `builtin/checksum.txt`

``` bash
cd builtin
find . -not -path checksum.txt -type f -exec sha256sum {} \;
```
