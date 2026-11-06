# Troubleshooting

| Message or symptom | Cause and fix |
| --- | --- |
| `The bundle name "..." must be a single file name` | `name` contains `/`, `\`, or is `.` or `..`. |
| `Unknown setting ...` when reading `Cargo.toml` | A key is misspelled or in the wrong table; unknown keys are errors at any depth. See [Manifest reference](manifest.md). |
| `No [package.metadata.bundle.bin.<name>] section` | `--bin` or `--example` tables are read instead of the main one; add the table. |
| `IPA packaging requires an iOS device target` | Use `--target aarch64-apple-ios`. |
| `iOS needs a bundle identifier` | Set `identifier`, using letters, digits and hyphens. |
| `APK packaging requires a [lib] target with crate-type = ["cdylib"]` | Add `[lib] crate-type = ["cdylib"]`, or pass a prebuilt `--binary-path <dir>/lib<name>.so`. |
| `APK packaging requires an Android target` | Add `--target aarch64-linux-android` (or another Android triple). |
| `All target triples must use the same operating system` / `only for universal macOS bundles` | Repeat `--target` only to combine macOS architectures. |
| `Use only one of [package.metadata.bundle.osx] or [...macos]` | They are one table; keep one. |
| `Failed to read localization file` / `Invalid localization file` | See [Translations in separate files](localization.md#translations-in-separate-files). |
| `Apple notarization is supported for macOS bundles and DMGs` | Remove `apple_notarization_api_key` for iOS formats. |
| `The APK is unsigned and cannot be installed` | Set `android.signing_key`. |
| `Invalid android.min_sdk ...` | The error names the setting that holds the bad value. |
| `Invalid locale code` / `both mean` | See [Locale codes](localization.md#locale-codes). |
| `Apple notarization requires ...` | Add the certificate, hardened runtime and timestamp. |
| `ELF machine type ... does not match` | `appimage_runtime` is for another CPU than `--target`. |
| `Failed to build the AppImage launcher` | Install the target's standard library (`rustup target add ...`) and linker, as for building the app itself, or set `appimage_runtime`. |
| `RPM packages are not implemented yet` | Use `deb` or `appimage`. |
| `built without the windows-signing feature` | Install with `--features windows-signing`. |
| `--format` value rejected | The error lists the valid formats; see [Formats](formats.md). |
| Bundle step builds the wrong package in a workspace | Pass `--package <name>`; it is forwarded to `cargo build`. |
| Missing resource | A glob that matches nothing adds nothing; check the pattern and run from the package directory. |
