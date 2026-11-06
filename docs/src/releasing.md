# Releasing from CI

Package each format on a runner that suits it, upload the results, and attach
them to a release. `dmg`, `deb`, `appimage` and `apk` can be built on any host,
and `exe` and `msi` embed icons and version data most completely on Windows.

```yaml
name: Package
on:
  push:
    tags: ["v*.*.*"]
  workflow_dispatch:

jobs:
  package:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: macos-latest, format: dmg, path: "target/release/bundle/dmg/*.dmg" }
          - { os: ubuntu-latest, format: deb, path: "target/release/bundle/deb/*.deb" }
          - { os: ubuntu-latest, format: appimage, path: "target/release/bundle/appimage/*.AppImage" }
          - { os: windows-latest, format: exe, path: "target/release/bundle/exe/*.exe" }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-bundle --locked
      - run: cargo bundle --release --format ${{ matrix.format }}
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.format }}
          path: ${{ matrix.path }}
          if-no-files-found: error

  release:
    if: startsWith(github.ref, 'refs/tags/')
    needs: package
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with: { path: dist, merge-multiple: true }
      - run: gh release create "$GITHUB_REF_NAME" dist/* --repo "$GITHUB_REPOSITORY" --verify-tag --generate-notes
        env:
          GH_TOKEN: ${{ github.token }}
```

Keep the `release` job's write permission to that job alone. Pin third-party
actions to reviewed commit hashes in your own workflow.

## Things to adapt

- **Paths.** Output goes to `<target-dir>/[<triple>/]<profile>/bundle/<format>/`;
  a custom `target-dir`, a `--target` or a `--profile` changes it. Several macOS
  targets use `universal`. Examples add `examples/`.
- **Workspaces.** `--package <name>` selects the member and is passed on to
  `cargo build`.
- **Building once.** Build with your own flags, then package from the result so
  nothing is compiled twice. `--binary-path` takes the executable, and one
  build can be packaged into several formats:

  ```sh
  cargo build --release
  cargo bundle --release --format deb --binary-path target/release/my-app
  cargo bundle --release --format appimage --binary-path target/release/my-app
  ```
- **AppImage.** The first AppImage build for a target compiles the small
  launcher, so the runner needs that target's standard library and a linker. See
  [Linux](platforms/linux.md#the-runtime).
- **Signing.** Keep certificates, passwords and keys in the CI secret store, set
  the keys described in [Signing](signing.md), and expose passwords through the
  environment variables those settings name. A short-lived Sigstore token comes
  from your CI provider's OIDC. Notarization runs in the same job as signing.
