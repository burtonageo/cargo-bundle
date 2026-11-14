# Linux

`deb` builds a Debian package and `appimage` builds a type-2 AppImage. `rpm` is
not implemented. Both formats are built in Rust and can be built on any host.

## Settings

These go in `[package.metadata.bundle.linux]` and are used by both formats.

| Key | Meaning |
| --- | --- |
| `mime_types` | MIME types for the `MimeType` key of the `.desktop` file. |
| `exec_args` | Text placed after the binary name in `Exec`, such as `"%f"`. |
| `use_terminal` | `true` for a console program (`Terminal=true`). Default `false`. |
| `startup_wm_class` | `StartupWMClass`, so desktops can match windows to the launcher. |
| `desktop_actions` | Extra launcher actions, described below. |
| `localizations` | Translated `.desktop` fields, described below. |

`deb_depends` (a top-level key) lists runtime packages for the `Depends:` field
of a `deb`, for example `["libgl1-mesa-glx", "libsdl2-2.0-0 (>= 2.0.5)"]`.

## Localizations

Each sub-table is a [locale code](../localization.md#locale-codes) holding
optional `Name`, `GenericName`, `Comment`, `Keywords` and `Icon` values. They
become `Name[fr]=...` lines. `Keywords` may be a list or a `;`-separated
string.

```toml
[package.metadata.bundle.linux.localizations.fr]
Name = "Mon App"
Comment = "Une description"
Keywords = ["outil", "utilitaire"]
```

The Desktop Entry spec wants an unlocalized value whenever a translated one
exists. `Name` and `Comment` come from `name` and `short_description`;
`GenericName` and `Keywords` fall back to the `C` locale, then `en`, then the
first locale in sorted order.

## Desktop actions

```toml
[package.metadata.bundle.linux.desktop_actions.new-window]
Name = "New Window"
Exec = "myapp --new-window"

[package.metadata.bundle.linux.desktop_actions.new-window.NameLocalized]
fr = "Nouvelle fenêtre"
```

`Name` is required, `Exec` defaults to the binary, and `Icon` is optional. An
action id may use letters, digits and `-`.

## AppImage

An AppImage is a runtime executable followed by a SquashFS image of the app.

| Key | Meaning |
| --- | --- |
| `appimage_metainfo_path` | An AppStream metainfo XML file, copied to `usr/share/metainfo/<identifier>.appdata.xml`. A warning is printed when it is missing. |
| `appimage_compression` | `"gzip"` (the default and most compatible), `"lz4"`, `"lzo"` or `"none"`; anything else is rejected when the manifest is read. |
| `appimage_runtime` | Path to the runtime executable. See below. |

Elements of the metainfo file marked `trkey="Name"` (or `Comment`,
`GenericName`) get `xml:lang` copies for each locale that translates them.

On a Linux host, shared libraries the executable links to are copied from the
host's `ld.so` cache into `usr/lib` unless they are on the AppImage exclude
list (libraries every desktop provides). On other hosts nothing is collected
and a warning says why.

### The runtime

The runtime is the executable at the front of the image. cargo-bundle uses the
pure-Rust launcher from [`appimage-rs`](https://crates.io/crates/appimage-rs):
nothing is downloaded and no prebuilt binary is involved. The first time you
package for a target, cargo-bundle builds the launcher with your own `cargo`
for that target, about half a megabyte, and caches it per `appimage-rs` version
and target under the user cache directory. Rebuilds reuse it.

The launcher does not mount the image, so users need no FUSE. When the image
runs it unpacks into a private temporary directory, starts `AppRun`, forwards
signals, and cleans up when the app exits. This needs disk space for the
unpacked app and a temporary directory that allows executing files.

Building the launcher needs what building your application for that target
already needs: the target's standard library and a linker. cargo resolves
`appimage-rs` offline when it can, because installing cargo-bundle fetched it.
Pass `--target` as usual; with no target the host's launcher is built, which
suits a Linux host.

`appimage_runtime` replaces the built launcher with a runtime you supply, for
packaging where the target cannot be built, such as a static musl launcher
(`cargo build --profile runtime --features launcher --bin appimage-runtime
--target x86_64-unknown-linux-musl` in `appimage-rs`). Either way the runtime
must be an ELF executable for the target CPU; one for a different architecture
is refused instead of producing an image that cannot start.
