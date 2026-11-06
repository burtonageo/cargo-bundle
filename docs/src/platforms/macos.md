# macOS

`osx` builds `<Name>.app`; `dmg` builds a disk image holding the app and a link
to `/Applications`. The disk image is written in Rust (HFS+ inside a compressed
UDIF container), so `dmg` also works on Linux and Windows hosts.

## Settings

All of these go in `[package.metadata.bundle.osx]` (`macos` is accepted as an
alias).

| Key | Meaning |
| --- | --- |
| `frameworks` | Frameworks to copy into `Contents/Frameworks`: a name such as `"SDL2"`, searched for in `~/Library/Frameworks`, `/Library/Frameworks` and `/Network/Library/Frameworks`, or a path to a `.framework`. You still link against them and set the rpath yourself. |
| `plugins` | Plugins to copy into the bundle. |
| `minimum_system_version` | `LSMinimumSystemVersion`, such as `"10.11"`. Also set `MACOSX_DEPLOYMENT_TARGET` in `build.rs` so the binary agrees. |
| `url_schemes` | URL schemes the app handles. |
| `info_plist_exts` | Paths of files whose contents are appended to `Info.plist` after the generated keys. |
| `localizations` | See [Localization](../localization.md). |
| `dmg_background` | An SVG used as the Finder window background of the `dmg`. |

## The disk image window

`dmg_background` must contain two elements with the ids `app` and
`applications`. The center of each becomes the position of the application and
of the Applications link, and the SVG's size becomes the window size. The image
is rendered at twice its size for Retina displays. `examples/hello` has a
working file:

```xml
<svg xmlns="http://www.w3.org/2000/svg" width="500" height="300">
  <rect id="app" x="80" y="120" width="96" height="96" fill="none"/>
  <rect id="applications" x="320" y="120" width="96" height="96" fill="none"/>
</svg>
```

## Universal binaries

Repeat `--target` to build each architecture and combine them with `lipo`:

```sh
cargo bundle -t aarch64-apple-darwin -t x86_64-apple-darwin
```

## Signing and notarization

See [Signing](../signing.md#apple). A signed `dmg` is signed twice: the app
inside first, then the image.
