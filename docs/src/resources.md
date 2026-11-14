# Resources and icons

`resources` holds files the program reads at run time; `icon` holds artwork. A
file may be in both.

```toml
[package.metadata.bundle]
resources = ["assets", "data/**/*.json", "config/default.toml"]
icon = ["icons/app.svg", "icons/app-128.png", "icons/app-256@2x.png"]
```

A resource pattern may match files or directories; directories are copied
recursively with their names kept (`assets/logo.png` stays under `assets/`). A
pattern that matches nothing adds nothing, so check the spelling when a file is
missing. Paths outside the package stay inside the bundle: a leading `/` becomes
`_root_` and each `..` becomes `_up_`.

## Where resources go

| Format | Destination |
| --- | --- |
| `osx` | `Contents/Resources/` |
| `ios` | the root of the `.app` |
| `apk` | `assets/` |
| `deb`, `appimage` | `usr/lib/<binary>/` |
| `msi` | `Resources/` beside the installed executable |
| `wxsmsi` | their source-relative directories below the install folder; keep them in a subdirectory such as `assets/` |
| `exe` | not copied |

Find them at run time relative to the executable, not the working directory: a
launcher can start the program from anywhere. cargo-bundle sets no environment
variable and offers no lookup API.

## Icons

`icon` is a list of files or globs. A name ending in `@2x` before the
extension is a high-density image where the platform has that convention.

| Input | macOS | iOS | Linux |
| --- | --- | --- | --- |
| SVG | the first SVG is rendered into the `.icns` | the first SVG is rendered at every iPhone and iPad size, 20px to 1024px | copied as the scalable icon |
| PNG | resized into the `.icns` | copied at its own size | installed at its own size |
| ICNS | used as is | unpacked to PNGs | unpacked to PNGs |
| other raster formats | converted | converted to PNG | converted to PNG |

Android uses the largest PNG as the launcher icon. On Windows, `msi` embeds the
first `.ico`, `wxsmsi` converts the first icon to `.ico`, and `exe` embeds an
SVG as the executable's icon when built on Windows. A corrupt image is an
error. PNG is the most portable choice.
