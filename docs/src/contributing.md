# Contributing

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Tests

Most tests run the real `cargo-bundle` binary on a throwaway project using
`--binary-path`, so they need no target toolchain, and read the output back
with an independent reader: `apple-codesign` for signatures, `zip` for IPAs,
`fstool` for disk images, `appimage-rs` for AppImages. `tests/common.rs` has the
shared helpers; they run with `TERM=dumb` so plain terminals stay covered.

When you add a behavior, add a test that fails without it. Break the code on
purpose and watch the test fail before trusting it. The AppImage and DMG tests
need no network because they use `appimage_runtime` and build images in memory.

`tests/docs.rs` checks that every setting and every format has a mention in this
guide and that its links resolve, so documentation cannot silently fall behind
the code.

## Building the guide

```sh
cargo install mdbook
mdbook build docs
```

The output is `docs/book`.

## Layout

- `src/bundle/<format>_bundle.rs`: one file per format.
- `src/bundle/settings.rs`: the manifest and the `Settings` accessors.
- `src/bundle/signing.rs`: Apple, Windows and Sigstore signing.
- `src/bundle/common.rs`: shared file, SVG and terminal helpers.
