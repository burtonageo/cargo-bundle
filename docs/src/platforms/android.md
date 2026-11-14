# Android

`apk` packages a NativeActivity shared library into a signed or unsigned
`<Name>.apk`. The library must be a Rust `cdylib` built for an Android target
that exports `ANativeActivity_onCreate`.

The package needs a `cdylib` target. `--target` must be an Android triple, and
cargo-bundle builds the library (`cargo build --lib`) and packages
`lib<name>.so`; `<name>` becomes the activity's library name.

```toml
[lib]
crate-type = ["cdylib"]
```

```sh
cargo bundle --release --format apk --target aarch64-linux-android
```

To package a library built elsewhere, pass it with `--binary-path`; its file
name must be `lib<name>.so`.

```toml
[package.metadata.bundle]
name = "My App"
identifier = "com.example.my_app"
resources = ["assets/*"]
icon = ["icons/icon-512.png"]

[package.metadata.bundle.android]
version_code = 1
min_sdk = 24
target_sdk = 35
permissions = ["android.permission.INTERNET"]
signing_key = "release.pem"
```

## Settings

| Key | Meaning |
| --- | --- |
| `version_code` | `android:versionCode`, default 1. The version name is the bundle version. |
| `min_sdk`, `target_sdk` | API levels. Without them the `rust-apk` defaults apply. |
| `permissions` | Extra manifest permissions such as `android.permission.INTERNET`. |
| `manifest` | A complete `AndroidManifest.xml` to use instead of the generated NativeActivity one, relative to the package's `Cargo.toml`. |
| `signing_key` | A PEM file holding the private key and certificate, relative to the package's `Cargo.toml`. |

Unknown keys under `[android]` and invalid values are errors that name the key.

## Details

- The `identifier` is the Android package name, so each dot-separated part
  needs letters, digits and underscores (a hyphen is not allowed).
- Resources become files under `assets/`. The largest PNG in `icon` becomes the
  launcher icon.
- The architecture comes from `--target`: aarch64, arm, x86 and x86_64. One
  invocation packages one ABI.
- Without `signing_key` the APK is unsigned and a warning says so; Android will
  not install it until it is signed.
- The APK is written in Rust, without the Android SDK, and an existing APK is
  kept if a rebuild fails.
