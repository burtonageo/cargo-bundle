//! AppDir layout construction for type-2 AppImages.

use super::context::AppImageContext;
use super::set_executable_permissions;
use crate::bundle::linux::desktop::{DesktopFileOptions, generate_desktop_file};
use crate::bundle::linux::icons::generate_icon_files;
use crate::bundle::localization::LinuxDesktopLocalizations;
use crate::bundle::{Settings, common};
use anyhow::Context;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
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

        // Bundle shared libraries alongside the executable.
        let usr_lib_directory = self.path.join("usr/lib");
        bundle_shared_libraries(&binary_absolute_destination, &usr_lib_directory)?;

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

    /// Copy AppStream metainfo XML into the AppDir, injecting `xml:lang` sibling
    /// elements for any element marked with a `trkey` attribute.
    ///
    /// When `linux_localizations` are configured, elements tagged with
    /// `trkey="Name"`, `trkey="Comment"`, or `trkey="GenericName"` get additional
    /// `<element xml:lang="…">…</element>` siblings for each locale that provides
    /// a non-empty translation.  The `trkey` attribute itself is stripped from the
    /// output so the final XML is valid AppStream.
    fn copy_metainfo(&self) -> crate::Result<()> {
        let Some(source_path) = self.settings.appimage_metainfo_path() else {
            let _ = common::print_warning(
                "No appimage_metainfo_path set. AppStream metainfo is recommended for \
                 discoverability in software centers.",
            );
            return Ok(());
        };

        let dest_dir = self.path.join("usr/share/metainfo");
        fs::create_dir_all(&dest_dir).with_context(|| {
            format!("Failed to create metainfo directory {}", dest_dir.display())
        })?;
        let destination =
            dest_dir.join(format!("{}.appdata.xml", self.settings.bundle_identifier()));

        let localizations = self.settings.linux_localizations();
        copy_metainfo_with_localizations(
            Path::new(source_path),
            &destination,
            localizations.as_ref(),
        )
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

/// Read an AppStream metainfo XML from `source`, optionally inject `xml:lang`
/// sibling elements for translatable tags (those carrying a `trkey` attribute),
/// and write the result to `destination`.
///
/// The `trkey` attribute value names a field in [`LinuxDesktopLocale`]
/// (`"Name"`, `"Comment"`, or `"GenericName"`).  For each locale that provides
/// a non-empty value for that field an additional element is emitted immediately
/// after the base element with `xml:lang="<locale>"`.  The `trkey` attribute is
/// stripped from the output so the produced file is valid AppStream XML.
///
/// When no localizations are configured the function falls back to a plain copy.
fn copy_metainfo_with_localizations(
    source: &Path,
    destination: &Path,
    localizations: Option<&LinuxDesktopLocalizations<'_>>,
) -> crate::Result<()> {
    let xml_source = fs::read_to_string(source)
        .with_context(|| format!("Failed to read metainfo file {}", source.display()))?;

    let mut reader = Reader::from_str(&xml_source);
    reader.config_mut().trim_text(false);

    let destination_file = common::create_file(destination)?;
    let mut writer = Writer::new_with_indent(destination_file, b' ', 2);

    // Stack tracking how deeply nested we are inside an element that had a
    // `trkey` attribute.  We suppress writing those elements' own content and
    // instead re-emit localized copies after the closing tag.
    struct PendingElement {
        tag: Vec<u8>,
        /// Reconstructed opening tag bytes (without the trkey attribute) for
        /// the base (unlocalized) element.
        opening_bytes: Vec<u8>,
        /// Content accumulated between the opening and closing tags.
        inner_text: String,
        /// The trkey value identifying which locale field to look up.
        translation_key: String,
        /// How many levels of nesting are inside this element.
        depth: u32,
    }

    let mut pending: Option<PendingElement> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(anyhow::Error::msg(format!(
                    "XML parse error in {}: {error}",
                    source.display()
                )));
            }

            Ok(Event::Start(element)) => {
                if let Some(ref mut pending_element) = pending {
                    // We are inside a trkey element — just accumulate depth.
                    pending_element.depth += 1;
                    continue;
                }

                // Check for a trkey attribute.
                let mut translation_key: Option<String> = None;
                let mut filtered_attributes: Vec<quick_xml::events::attributes::Attribute<'_>> =
                    Vec::new();
                for attribute_result in element.attributes() {
                    let attribute = attribute_result.with_context(|| {
                        format!("Invalid XML attribute in {}", source.display())
                    })?;
                    if attribute.key.as_ref() == b"trkey" {
                        translation_key =
                            Some(String::from_utf8_lossy(&attribute.value).into_owned());
                    } else {
                        filtered_attributes.push(attribute);
                    }
                }

                if let Some(key) = translation_key {
                    // Build the opening tag bytes without trkey for later replay.
                    let mut opening = BytesStart::new(
                        String::from_utf8_lossy(element.name().as_ref()).into_owned(),
                    );
                    for attribute in &filtered_attributes {
                        opening.push_attribute((attribute.key.as_ref(), attribute.value.as_ref()));
                    }
                    let mut opening_bytes = Vec::new();
                    {
                        let mut temp = Writer::new(&mut opening_bytes);
                        temp.write_event(Event::Start(opening))
                            .map_err(anyhow::Error::msg)?;
                    }
                    pending = Some(PendingElement {
                        tag: element.name().as_ref().to_owned(),
                        opening_bytes,
                        inner_text: String::new(),
                        translation_key: key,
                        depth: 0,
                    });
                } else {
                    // Ordinary element — pass through as-is.
                    let mut passthrough = BytesStart::new(
                        String::from_utf8_lossy(element.name().as_ref()).into_owned(),
                    );
                    for attribute in &filtered_attributes {
                        passthrough
                            .push_attribute((attribute.key.as_ref(), attribute.value.as_ref()));
                    }
                    writer
                        .write_event(Event::Start(passthrough))
                        .map_err(anyhow::Error::msg)?;
                }
            }

            Ok(Event::End(element)) => {
                if let Some(ref mut pending_element) = pending {
                    if pending_element.depth > 0 {
                        pending_element.depth -= 1;
                        continue;
                    }
                    // Closing the trkey element — emit base + localized copies.
                    let tag_name = String::from_utf8_lossy(&pending_element.tag).into_owned();
                    let inner_text = pending_element.inner_text.clone();
                    let translation_key = pending_element.translation_key.clone();
                    let opening_bytes = pending_element.opening_bytes.clone();
                    pending = None;

                    // Emit the base (unlocalized) element.
                    writer
                        .get_mut()
                        .write_all(&opening_bytes)
                        .map_err(anyhow::Error::msg)?;
                    writer
                        .write_event(Event::Text(BytesText::new(&inner_text)))
                        .map_err(anyhow::Error::msg)?;
                    writer
                        .write_event(Event::End(BytesEnd::new(tag_name.clone())))
                        .map_err(anyhow::Error::msg)?;

                    // Emit one localized sibling per locale.
                    if let Some(locs) = localizations {
                        for (locale, locale_entry) in locs.sorted_locales() {
                            let translation = match translation_key.as_str() {
                                "Name" => locale_entry.name.as_deref(),
                                "Comment" => locale_entry.comment.as_deref(),
                                "GenericName" => locale_entry.generic_name.as_deref(),
                                _ => None,
                            };
                            let Some(text) = translation.filter(|t| !t.is_empty()) else {
                                continue;
                            };
                            let mut localized_start = BytesStart::new(tag_name.clone());
                            localized_start.push_attribute(("xml:lang", locale));
                            writer
                                .write_event(Event::Start(localized_start))
                                .map_err(anyhow::Error::msg)?;
                            writer
                                .write_event(Event::Text(BytesText::new(text)))
                                .map_err(anyhow::Error::msg)?;
                            writer
                                .write_event(Event::End(BytesEnd::new(tag_name.clone())))
                                .map_err(anyhow::Error::msg)?;
                        }
                    }
                } else {
                    writer
                        .write_event(Event::End(element))
                        .map_err(anyhow::Error::msg)?;
                }
            }

            Ok(Event::Text(text)) => {
                if let Some(ref mut pending_element) = pending {
                    pending_element
                        .inner_text
                        .push_str(&String::from_utf8_lossy(&text));
                } else {
                    writer
                        .write_event(Event::Text(text))
                        .map_err(anyhow::Error::msg)?;
                }
            }

            Ok(other_event) => {
                if pending.is_none() {
                    writer
                        .write_event(other_event)
                        .map_err(anyhow::Error::msg)?;
                }
            }
        }
    }

    Ok(())
}

/// Lines from the AppImage community excludelist of libraries assumed present on all hosts.
#[cfg(target_os = "linux")]
const APPIMAGE_EXCLUDELIST: &str = include_str!("appimage-excludelist");

/// Walk the ELF dynamic-link graph starting from `executable`, resolve every
/// soname to a real path via `/etc/ld.so.cache`, skip sonames that appear in
/// the excludelist, and copy each resolved library flat into `destination_directory`.
///
/// If `/etc/ld.so.cache` cannot be read (e.g. building on macOS) a warning is
/// logged and the function returns successfully so cross-compilation builds are
/// not broken.
#[cfg(target_os = "linux")]
fn bundle_shared_libraries(executable: &Path, destination_directory: &Path) -> crate::Result<()> {
    use std::collections::{HashMap, HashSet};

    let cache_bytes = match fs::read("/etc/ld.so.cache") {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = common::print_warning(&format!(
                "Could not read /etc/ld.so.cache ({error}); shared libraries will not be bundled."
            ));
            return Ok(());
        }
    };

    let parsed_cache =
        ld_so_cache::parsers::parse_ld_cache(&cache_bytes).map_err(anyhow::Error::msg)?;
    let cache_entries = parsed_cache.get_entries().map_err(anyhow::Error::msg)?;

    let excluded_sonames: Vec<&str> = APPIMAGE_EXCLUDELIST
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    fs::create_dir_all(destination_directory).with_context(|| {
        format!(
            "Failed to create shared-library directory {}",
            destination_directory.display()
        )
    })?;

    // Work queue of ELF files whose DT_NEEDED entries we still need to process.
    let mut pending: Vec<PathBuf> = vec![executable.to_owned()];
    // Absolute paths of libraries already scheduled for copying (avoids cycles).
    let mut resolved: HashSet<PathBuf> = HashSet::new();
    // Sonames that appeared in DT_NEEDED but had no entry in the cache.
    let mut unresolved: HashMap<String, Vec<PathBuf>> = HashMap::new();

    while let Some(elf_path) = pending.pop() {
        for soname in elf_dynamic_dependencies(&elf_path)? {
            if excluded_sonames.contains(&soname.as_str()) {
                continue;
            }

            let Some(cache_entry) = cache_entries
                .iter()
                .find(|entry| entry.library_name == soname)
            else {
                unresolved.entry(soname).or_default().push(elf_path.clone());
                continue;
            };

            let library_path = PathBuf::from(&cache_entry.library_path);
            if resolved.insert(library_path.clone()) {
                pending.push(library_path);
            }
        }
    }

    if !unresolved.is_empty() {
        for (soname, required_by) in &unresolved {
            let _ = common::print_warning(&format!(
                "Shared library '{soname}' not found in ld.so.cache (required by: {}).",
                required_by
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let _ = common::print_warning(
            "The above libraries could not be resolved; the AppImage bundle may be incomplete.",
        );
    }

    for library_path in &resolved {
        let filename = library_path
            .file_name()
            .with_context(|| format!("Library path has no filename: {}", library_path.display()))?;
        let destination = destination_directory.join(filename);
        fs::copy(library_path, &destination).with_context(|| {
            format!(
                "Failed to copy shared library {} to {}",
                library_path.display(),
                destination.display()
            )
        })?;
    }

    Ok(())
}

/// Return the list of sonames (DT_NEEDED entries) from an ELF file's dynamic
/// section. Returns an empty list if the file is not ELF or has no dynamic section.
#[cfg(target_os = "linux")]
fn elf_dynamic_dependencies(elf_path: &Path) -> crate::Result<Vec<String>> {
    let bytes = fs::read(elf_path)
        .with_context(|| format!("Failed to read ELF file {}", elf_path.display()))?;

    let elf = match goblin::elf::Elf::parse(&bytes) {
        Ok(elf) => elf,
        Err(_) => return Ok(vec![]),
    };

    Ok(elf
        .libraries
        .iter()
        .map(|soname| soname.to_string())
        .collect())
}

#[cfg(not(target_os = "linux"))]
fn bundle_shared_libraries(_executable: &Path, _destination_directory: &Path) -> crate::Result<()> {
    Ok(())
}

fn parse_icon_directory_area(name: &str) -> Option<u64> {
    let base_name = name.split('@').next().unwrap_or(name);
    let mut dimensions = base_name.split('x');
    let width: u64 = dimensions.next()?.parse().ok()?;
    let height: u64 = dimensions.next()?.parse().ok()?;
    Some(width.saturating_mul(height))
}

#[cfg(test)]
mod tests {
    use super::{copy_metainfo_with_localizations, parse_icon_directory_area};
    use crate::bundle::localization::{LinuxDesktopLocale, LinuxDesktopLocalizations};
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_icon_dir_area_basic() {
        assert_eq!(parse_icon_directory_area("256x256"), Some(65536));
        assert_eq!(parse_icon_directory_area("128x128@2x"), Some(16384));
        assert_eq!(parse_icon_directory_area("scalable"), None);
    }

    #[test]
    fn metainfo_passthrough_without_localizations() {
        let source = NamedTempFile::new().unwrap();
        std::fs::write(
            source.path(),
            r#"<?xml version="1.0"?><component><name>My App</name></component>"#,
        )
        .unwrap();
        let destination = NamedTempFile::new().unwrap();
        copy_metainfo_with_localizations(source.path(), destination.path(), None).unwrap();
        let output = std::fs::read_to_string(destination.path()).unwrap();
        assert!(output.contains("<name>My App</name>"));
    }

    #[test]
    fn metainfo_injects_xml_lang_siblings() {
        let source = NamedTempFile::new().unwrap();
        std::fs::write(
            source.path(),
            r#"<?xml version="1.0"?><component><name trkey="Name">My App</name></component>"#,
        )
        .unwrap();
        let destination = NamedTempFile::new().unwrap();

        let mut locale_map = HashMap::new();
        locale_map.insert(
            "fr".to_string(),
            LinuxDesktopLocale {
                name: Some("Mon App".to_string()),
                comment: None,
                generic_name: None,
                keywords: None,
                icon: None,
            },
        );
        let localizations = LinuxDesktopLocalizations::new(&locale_map);
        copy_metainfo_with_localizations(source.path(), destination.path(), Some(&localizations))
            .unwrap();

        let output = std::fs::read_to_string(destination.path()).unwrap();
        // Base element is present without trkey.
        assert!(
            output.contains("<name>My App</name>")
                || output.contains("<name>\nMy App\n</name>")
                || output.contains("My App")
        );
        // Localized sibling is present.
        assert!(output.contains("xml:lang=\"fr\""));
        assert!(output.contains("Mon App"));
        // trkey attribute must not appear in output.
        assert!(!output.contains("trkey"));
    }
}
