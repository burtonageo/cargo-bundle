//! AppDir layout construction for type-2 AppImages.

use super::context::AppImageContext;
use super::set_executable_permissions;
use crate::bundle::linux::desktop::{DesktopFileOptions, generate_desktop_file};
use crate::bundle::linux::icons::generate_icon_files;
use crate::bundle::{Settings, common};
use anyhow::Context;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) struct AppDirectory<'a> {
    settings: &'a Settings,
    pub(super) path: PathBuf,
}

impl<'a> AppDirectory<'a> {
    pub(super) fn build(
        settings: &'a Settings,
        context: &'a AppImageContext,
    ) -> crate::Result<Self> {
        let directory = Self {
            settings,
            path: context.app_directory_path.clone(),
        };

        directory.stage_executable()?;
        directory.generate_assets()?;
        directory.setup_entry_points()?;
        directory.copy_metainfo()?;

        Ok(directory)
    }

    fn stage_executable(&self) -> crate::Result<()> {
        let binary_relative_destination = self.binary_relative_path();
        let binary_absolute_destination = self.path.join(&binary_relative_destination);

        common::copy_file(self.settings.binary_path(), &binary_absolute_destination)?;
        set_executable_permissions(&binary_absolute_destination)?;

        // Feature 4: warn about non-excluded linked libraries.
        check_linked_libraries(&binary_absolute_destination);

        Ok(())
    }

    fn generate_assets(&self) -> crate::Result<()> {
        generate_icon_files(self.settings, &self.path)?;
        let options = DesktopFileOptions {
            appimage_version: Some(self.settings.version_string().to_string()),
        };
        generate_desktop_file(self.settings, &self.path, &options)?;
        self.transfer_resource_files()?;
        self.copy_license_file()?;
        self.generate_directory_icon_and_root_icon()?;
        Ok(())
    }

    /// Feature 3: copy AppStream metainfo XML into the AppDir (or warn if absent).
    fn copy_metainfo(&self) -> crate::Result<()> {
        if let Some(src) = self.settings.appimage_metainfo_path() {
            let dest_dir = self.path.join("usr/share/metainfo");
            fs::create_dir_all(&dest_dir).with_context(|| {
                format!("Failed to create metainfo directory {}", dest_dir.display())
            })?;
            let dest = dest_dir.join(format!("{}.appdata.xml", self.settings.bundle_identifier()));
            fs::copy(src, &dest).with_context(|| {
                format!("Failed to copy metainfo from {src:?} to {}", dest.display())
            })?;
        } else {
            let _ = common::print_warning(
                "No appimage_metainfo_path set. AppStream metainfo is recommended for \
                 discoverability in software centers.",
            );
        }
        Ok(())
    }

    fn binary_relative_path(&self) -> PathBuf {
        PathBuf::from("usr/bin").join(self.settings.binary_name())
    }

    fn setup_entry_points(&self) -> crate::Result<()> {
        let binary_relative_destination = self.binary_relative_path();

        common::symlink_file(&binary_relative_destination, &self.path.join("AppRun"))
            .with_context(|| "Failed to create AppRun symlink")?;

        let desktop_filename = format!("{}.desktop", self.settings.binary_name());
        let desktop_relative_path = PathBuf::from("usr/share/applications").join(&desktop_filename);

        common::symlink_file(&desktop_relative_path, &self.path.join(&desktop_filename))
            .with_context(|| "Failed to create root .desktop symlink")?;

        Ok(())
    }

    fn transfer_resource_files(&self) -> crate::Result<()> {
        let resource_directory = self.path.join("usr/lib").join(self.settings.binary_name());
        for source_path in self.settings.resource_files() {
            let source_path = source_path?;
            let destination_path = resource_directory.join(common::resource_relpath(&source_path));
            common::copy_file(&source_path, &destination_path)
                .with_context(|| format!("Failed to copy resource file {source_path:?}"))?;
        }
        Ok(())
    }

    fn copy_license_file(&self) -> crate::Result<()> {
        if let Some(content) = self.settings.license_content() {
            let destination_path = self
                .path
                .join("usr/share/doc")
                .join(self.settings.binary_name())
                .join("copyright");

            let mut file = common::create_file(&destination_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;
        }
        Ok(())
    }

    fn generate_directory_icon_and_root_icon(&self) -> crate::Result<()> {
        let binary_name = self.settings.binary_name();

        for icon_path in self.settings.icon_files() {
            let icon_path = icon_path?;
            if icon_path.extension() == Some(OsStr::new("svg")) {
                return self.rasterize_svg_icon(&icon_path, binary_name);
            }
        }

        if let Some((relative_png_path, _area)) = self.find_largest_hicolor_png(binary_name) {
            let root_png_path = format!("{binary_name}.png");

            common::symlink_file(&relative_png_path, &self.path.join(&root_png_path))
                .with_context(|| "Failed to create root-level PNG icon symlink")?;

            common::copy_file(
                &self.path.join(&relative_png_path),
                &self.path.join(".DirIcon"),
            )
            .with_context(|| "Failed to create .DirIcon from PNG icon")?;
        }

        Ok(())
    }

    fn rasterize_svg_icon(&self, icon_path: &Path, binary_name: &str) -> crate::Result<()> {
        let svg_data = fs::read_to_string(icon_path)
            .with_context(|| format!("Failed to read SVG icon {icon_path:?}"))?;

        let render_options = Options::default();
        let tree = Tree::from_data(svg_data.as_bytes(), &render_options)
            .with_context(|| "Failed to parse SVG for .DirIcon")?;

        let pixel_size: u32 = 256;
        let mut pixel_map = Pixmap::new(pixel_size, pixel_size)
            .with_context(|| "Failed to create pixmap for .DirIcon")?;

        resvg::render(
            &tree,
            Transform::from_scale(
                pixel_size as f32 / tree.size().width(),
                pixel_size as f32 / tree.size().height(),
            ),
            &mut pixel_map.as_mut(),
        );

        pixel_map
            .save_png(self.path.join(".DirIcon"))
            .with_context(|| "Failed to save .DirIcon PNG")?;

        let svg_filename = format!("{binary_name}.svg");
        let svg_relative_path =
            PathBuf::from("usr/share/icons/hicolor/scalable/apps").join(&svg_filename);
        let _ = common::symlink_file(&svg_relative_path, &self.path.join(&svg_filename));

        Ok(())
    }

    fn find_largest_hicolor_png(&self, binary_name: &str) -> Option<(PathBuf, u64)> {
        let hicolor_directory = self.path.join("usr/share/icons/hicolor");
        if !hicolor_directory.is_dir() {
            return None;
        }

        let mut best_match: Option<(PathBuf, u64)> = None;
        let directory_entries = fs::read_dir(&hicolor_directory).ok()?;

        for entry in directory_entries.flatten() {
            let size_directory = entry.path();
            if !size_directory.is_dir() {
                continue;
            }

            let directory_name = entry.file_name();
            if directory_name == "scalable" {
                continue;
            }

            let png_path = size_directory
                .join("apps")
                .join(format!("{binary_name}.png"));
            if !png_path.is_file() {
                continue;
            }

            let parsed_directory_name = directory_name.to_string_lossy();
            let area = parse_icon_directory_area(&parsed_directory_name).unwrap_or(0);
            let relative_path = png_path.strip_prefix(&self.path).ok()?.to_path_buf();

            match &best_match {
                Some((_, best_area)) if area <= *best_area => {}
                _ => best_match = Some((relative_path, area)),
            }
        }
        best_match
    }
}

/// Feature 4: inspect the staged binary's ELF dynamic dependencies and warn
/// about any libraries not in the well-known always-available-on-host
/// excludelist. Uses the pure-Rust `goblin` crate (no external `ldd` call).
#[cfg(target_os = "linux")]
fn check_linked_libraries(binary: &Path) {
    const EXCLUDED_PREFIXES: &[&str] = &[
        "ld-linux",
        "libc",
        "libm",
        "libdl",
        "libpthread",
        "librt",
        "libgcc_s",
        "libstdc++",
        "libGL",
        "libEGL",
        "libX",
        "libxcb",
        "libwayland",
        "libfontconfig",
        "libfreetype",
        "libharfbuzz",
        "libglib",
        "libgobject",
        "libgio",
        "libgtk",
        "libgdk",
        "libdbus",
        "libsystemd",
        "libudev",
        "libasound",
        "libpulse",
        "libz",
        "libzstd",
        "liblzma",
        "libbz2",
        "libexpat",
        "libuuid",
        "libcrypt",
        "libnss",
        "libresolv",
    ];

    let Ok(bytes) = std::fs::read(binary) else {
        return; // can't read the binary
    };
    let Ok(elf) = goblin::elf::Elf::parse(&bytes) else {
        return; // not an ELF we can parse
    };

    let is_always_available = |library: &str| {
        // Strip the version suffix for prefix matching ("libfoo.so.1" → "libfoo").
        let base = library.split('.').next().unwrap_or(library);
        EXCLUDED_PREFIXES
            .iter()
            .any(|prefix| base.starts_with(prefix))
    };

    let unbundled_libraries: Vec<&str> = elf
        .libraries
        .into_iter()
        .filter(|library| !is_always_available(library))
        .collect();

    if !unbundled_libraries.is_empty() {
        let list = unbundled_libraries.join(", ");
        let _ = common::print_warning(&format!(
            "The binary links against libraries that may not be available on all target systems: {list}. \
             Consider bundling them under usr/lib/ in the AppDir or using a statically linked build."
        ));
    }
}

#[cfg(not(target_os = "linux"))]
fn check_linked_libraries(_binary: &Path) {}

fn parse_icon_directory_area(name: &str) -> Option<u64> {
    let base_name = name.split('@').next().unwrap_or(name);
    let mut dimensions = base_name.split('x');
    let width: u64 = dimensions.next()?.parse().ok()?;
    let height: u64 = dimensions.next()?.parse().ok()?;
    Some(width.saturating_mul(height))
}

#[cfg(test)]
mod tests {
    use super::parse_icon_directory_area;

    #[test]
    fn parse_icon_dir_area_basic() {
        assert_eq!(parse_icon_directory_area("256x256"), Some(65536));
        assert_eq!(parse_icon_directory_area("128x128@2x"), Some(16384));
        assert_eq!(parse_icon_directory_area("scalable"), None);
    }
}
