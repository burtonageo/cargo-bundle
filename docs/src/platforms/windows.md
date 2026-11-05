# Windows

`exe` copies the executable, `msi` writes an installer in Rust, and `wxsmsi`
generates a WiX project and builds it with `dotnet`. Both installer writers are
experimental; test installation, removal and upgrades on every Windows version
you support.

Choose the target when cross-compiling, for example
`--target x86_64-pc-windows-msvc`. Without `--format`, a Windows target builds
`msi` and `exe`.

A `[package.metadata.bundle.windows]` table has no keys of its own, but it can
replace general settings such as `name` or `icon` for Windows builds; see
[Platform tables](../manifest.md#platform-tables).

## exe

Copies the program to `bundle/exe/<binary>.exe`. On a Windows host it also
writes version information (name, version, description, copyright, authors) and
embeds an icon, but only from an SVG, rendered at several sizes. On other hosts
the resource editing is skipped with a warning. Resources are not copied beside
the executable.

## msi

Writes `bundle/msi/<Name>.msi`: the executable under `Program Files/<Name>` and
[resources](../resources.md) under `Resources/`. The product code is derived
from `identifier`, so set a stable one. The writer does not author upgrade
sequencing or shortcuts; do not rely on it to upgrade an existing installation.
Use a plain numeric `version` such as `1.2.3`. The first `.ico` in `icon` is
stored in the package's Icon table (not shown in Add/Remove Programs).

## wxsmsi

Writes `bundle/wxsmsi/installer.wixproj` and runs `dotnet build`, which needs to
restore `WixToolset.Sdk` and `WixToolset.UI.wixext` (6.0.2) from NuGet. The
installer is in `bundle/wxsmsi/bin/<Release|Debug>/` named after the bundle in
lowercase with hyphens (`My App` becomes `my-app.msi`). It adds Start Menu and
Desktop shortcuts, a license page, an install-folder page and a launch
checkbox, and includes a major-upgrade rule.

Keep resources in a subdirectory such as `assets/`: a file at the package root
makes the generated project fail to link. Paths outside the package are not
supported. The first icon is used if it is an `.ico`, `.exe` or `.dll`, or an
image that can be converted; SVG is not converted, and later icons are not
tried.

The license page shows `license-file` from `Cargo.toml` if set, otherwise the
`license` text, otherwise a license file found in the working directory.

## Signing

See [Signing](../signing.md#windows).
