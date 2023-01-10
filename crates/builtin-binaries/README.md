# gw-builtin-binaries

This crate contains builtin binaries from `/gwos` and `/gwos-evm`.

## Usage

The `build.rs` file contains checksum and path of binaries, edit the file to add new binaries or update the bundled file path.

## Release

This crate is supposed to be separately released, so we put it in its own cargo workspace.

To release a new version of `gw-builtin-binaries`:

1. Edit builtin binaries in `build.rs`.
2. Run `cargo publish --dry-run --alow-dirty` locally to check the build process.
3. Publish a new tag `builtin-binaries-v*.*.*`, which will trigger a GitHub action(`.github/workflows/publish-builtin-binaries.yml`) to run the publishing process
