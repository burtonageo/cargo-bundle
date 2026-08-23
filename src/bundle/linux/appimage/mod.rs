//! Type-2 AppImage bundler.
//!
//! # Format
//!
//! A type-2 AppImage is:
//!   `[ELF type2 runtime][SquashFS 4.0 payload]`
//!
//! The runtime (from the AppImage project) mounts the SquashFS with FUSE and
//! executes `AppRun` at the root of the resulting AppDir. The payload is a
//! standard little-endian SquashFS 4.0 image; gzip compression is used for
//! maximum compatibility with older kernels / squashfuse builds.
//!
//! # SquashFS strategy
//!
//! SquashFS images are written with the pure-Rust [`backhand`] crate (gzip by
//! default, zstd when `appimage_compression = "zstd"`). No external `mksquashfs`
//! binary is required.
//!
//! # Layout
//!
//! ```text
//! AppDir/
//!   AppRun                      -> usr/bin/<bin>
//!   <bin>.desktop               -> usr/share/applications/<bin>.desktop
//!   <bin>.png | <bin>.svg       root icon (basename matches Icon=)
//!   .DirIcon                    PNG thumbnail
//!   usr/bin/<bin>
//!   usr/share/applications/<bin>.desktop
//!   usr/share/icons/hicolor/...
//!   usr/lib/<bin>/...           bundled resources (if any)
//!   usr/share/doc/<bin>/copyright
//! ```

mod app_dir;
mod arch;
mod context;
mod runtime;
mod squashfs;

use crate::bundle::{Settings, common};
use app_dir::AppDirectory;
use context::AppImageContext;
use std::path::PathBuf;

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let context = AppImageContext::new(settings)?;

    let package_name = context
        .package_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    common::print_bundling(&package_name)?;

    context.clean_previous_build()?;

    let app_directory = AppDirectory::build(settings, &context)?;
    let runtime_bytes = runtime::fetch(settings, context.architecture)?;

    squashfs::create_and_assemble(
        settings,
        &app_directory.path,
        &runtime_bytes,
        &context.package_path,
    )?;

    set_executable_permissions(&context.package_path)?;

    Ok(vec![context.package_path.clone()])
}

fn set_executable_permissions(path: &std::path::Path) -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::arch::determine_appimage_architecture;

    #[test]
    fn maps_common_cargo_arches() {
        assert_eq!(determine_appimage_architecture("x86_64").unwrap(), "x86_64");
        assert_eq!(
            determine_appimage_architecture("aarch64").unwrap(),
            "aarch64"
        );
        assert_eq!(determine_appimage_architecture("arm64").unwrap(), "aarch64");
        assert_eq!(determine_appimage_architecture("i686").unwrap(), "i686");
        assert_eq!(determine_appimage_architecture("x86").unwrap(), "i686");
        assert_eq!(determine_appimage_architecture("arm").unwrap(), "armhf");
        assert_eq!(determine_appimage_architecture("armv7").unwrap(), "armhf");
    }

    #[test]
    fn rejects_unsupported_arches() {
        assert!(determine_appimage_architecture("riscv64").is_err());
        assert!(determine_appimage_architecture("wasm32").is_err());
    }
}
