# Cargo bundle

[![Crates.io](https://img.shields.io/crates/v/cargo-bundle.svg)](https://crates.io/crates/cargo-bundle)
[![Build Status](https://github.com/burtonageo/cargo-bundle/workflows/CI/badge.svg?branch=master)](https://github.com/burtonageo/cargo-bundle/actions?query=branch%3Amaster)

Wrap Rust executables in OS-specific app bundles.

## About

`cargo-bundle` packages Cargo applications for distribution. It supports
macOS app bundles and disk images, iOS app bundles and IPAs, Debian packages
and AppImages, Windows executables and MSI installers, and Android APKs for
NativeActivity libraries. iOS and Windows support is experimental. RPM
packaging is not implemented.

Install it with `cargo install cargo-bundle`, which adds a `cargo bundle`
subcommand.

## Quick start

Describe the application in your `Cargo.toml`:

```toml
[package.metadata.bundle]
name = "My App"
identifier = "com.example.my-app"
icon = ["icons/icon.png"]
```

Then, from the package directory:

```sh
cargo bundle --release
```

This builds every default format for your operating system and prints where
the files went. Use `--format` to pick one, `--target` to cross-compile, and
`--binary-path` to package an executable you already built.

```text
-f, --format <FORMAT>  apk, deb, ios, ipa, msi, wxsmsi, osx, dmg, rpm, appimage, exe
```

## Documentation

The guide covers the command line, every manifest setting, each platform and
signing: start at [`docs/src/index.md`](docs/src/index.md), or build it with
`mdbook build docs`.

## Contributing

`cargo-bundle` has ambitions to be an inclusive project and welcomes
contributions from anyone. Please abide by the Rust code of conduct, and see
[CONTRIBUTING.md](CONTRIBUTING.md).

## Status

Early alpha. Expect the format of the `[package.metadata.bundle]` section to
change; there is no guarantee of stability.

## License

This program is licensed either under the terms of the
[Apache Software License](http://www.apache.org/licenses/LICENSE-2.0), or the
[MIT License](https://opensource.org/licenses/MIT).
