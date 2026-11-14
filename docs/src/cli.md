# Command line

```text
cargo bundle [OPTIONS]
```

| Option | Meaning |
| --- | --- |
| `-b, --bin <NAME>` | Bundle this binary. Its settings come from `[package.metadata.bundle.bin.<NAME>]`. |
| `-e, --example <NAME>` | Bundle this example, using `[package.metadata.bundle.example.<NAME>]`. |
| `-p, --package <SPEC>` | Bundle this workspace member. Without it the root package is used. |
| `-f, --format <FORMAT>` | One of the [formats](formats.md). |
| `-r, --release`, `--profile <NAME>` | Build and bundle that profile. |
| `-t, --target <TRIPLE>` | Build for a target. Repeat it on macOS to combine architectures into one universal binary with `lipo`; targets of different operating systems, or repeated non-macOS targets, are refused. |
| `--features`, `--all-features`, `--no-default-features` | Passed to `cargo build`. |
| `--binary-path <PATH>` | Bundle this prebuilt file instead of running `cargo build`. |

## Prebuilt binaries

`--binary-path` skips the build. The file is copied into the bundle and never
modified. Cargo metadata is still read, and `--release`, `--profile` and
`--target` still choose the output directory and the target's properties, but
they do not rebuild anything.

```sh
cargo build --release --bin my-app
cargo bundle --release --bin my-app --binary-path target/release/my-app
```

Use it when a CI job builds once and packages several formats, or when another
build system produced the executable. For an APK it names the prebuilt
`lib<name>.so`; see [Android](platforms/android.md).

## Output directories

Bundles go to `<target-dir>/[<triple>/]<profile>/bundle/<format>/`. Examples
add an `examples/` component before `bundle`. The development profile is
`debug`.

## Failures

A failed run leaves any earlier output of the same name in place for `dmg`,
`ipa`, `appimage` and `apk`: the new file is written elsewhere and moved into
place only when it is complete.
