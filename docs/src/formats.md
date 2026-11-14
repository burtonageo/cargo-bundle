# Formats

| Format | Output | Notes |
| --- | --- | --- |
| `osx` | `bundle/osx/<Name>.app` | Copying frameworks runs `install_name_tool`. |
| `dmg` | `bundle/dmg/<Name>.dmg` | Built in Rust; works on any host. |
| `ios` | `bundle/ios/<Name>.app` | Experimental. |
| `ipa` | `bundle/ipa/<Name>.ipa` | Needs `--target aarch64-apple-ios`. |
| `apk` | `bundle/apk/<Name>.apk` | Needs `--binary-path lib<name>.so`. |
| `deb` | `bundle/deb/<binary>_<version>_<arch>.deb` | You declare runtime dependencies. |
| `appimage` | `bundle/appimage/<binary>_<version>_<arch>.AppImage` | |
| `msi` | `bundle/msi/<Name>.msi` | Experimental native writer. |
| `wxsmsi` | `bundle/wxsmsi/` | Experimental; runs `dotnet build` on a generated WiX project. |
| `exe` | `bundle/exe/<binary>.exe` | A copy of the executable with Windows metadata. |
| `rpm` | none | Not implemented; the command fails with an error. |

## Defaults

Without `--format`, every default for the target operating system is built.
A `--target` triple decides the operating system; otherwise the host does.

| Target OS | Formats built |
| --- | --- |
| macOS | `osx`, `dmg` |
| iOS | `ios` |
| Android | `apk` |
| Linux | `deb`, `appimage` |
| Windows | `msi`, `exe` |

## Cross-compiling

`--target` picks the architecture to compile for. It does not install a
compiler, SDK or linker, and `--format` does not pick a target. Use both when
cross-compiling, and check the host requirements in the table: `dmg`, `ipa`,
`apk` and `appimage` need nothing but cargo-bundle, while `osx` frameworks and
universal binaries need macOS tools.
