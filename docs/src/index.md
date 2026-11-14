# Introduction

`cargo-bundle` turns a Rust executable into something you can hand to users: a
macOS `.app` or `.dmg`, an iOS `.app` or `.ipa`, an Android `.apk`, a Debian
`.deb`, a Linux `.AppImage`, or a Windows `.msi` or `.exe`. It reads its
settings from your `Cargo.toml`, so a release build is one command.

## Install

```sh
cargo install cargo-bundle
```

This adds a `cargo bundle` subcommand. Everything is built in except Windows
Authenticode signing, which is a cargo feature because of its license; see
[Signing](signing.md#windows).

## Your first bundle

Add a `[package.metadata.bundle]` table to `Cargo.toml`. Set `identifier`; the
package `description` becomes the short description:

```toml
[package]
name = "my-app"
version = "0.1.0"
description = "A small example application."

[package.metadata.bundle]
name = "My App"
identifier = "com.example.my-app"
```

Then, from the package directory:

```sh
cargo bundle --release
```

Without `--format`, cargo-bundle builds every default format for the target
operating system (see [Formats](formats.md)) and prints where each file went.
Output lands under Cargo's target directory:

```text
target/release/bundle/<format>/
target/<triple>/release/bundle/<format>/     (with --target)
```

For cross-compiled or separately built binaries, pass the executable with
`--binary-path`; see [Command line](cli.md).

## What is stable

The manifest format is still evolving. Unknown keys are errors, so a typo is
reported rather than silently ignored.
