// An OSX package is laid out like:
//
// foobar.app    # Actually a directory
//     Contents      # A further subdirectory
//         Info.plist     # An xml file containing the app's metadata
//         MacOS          # A directory to hold executable binary files
//             foobar          # The main binary executable of the app
//             foobar_helper   # A helper application, possibly provitidng a CLI
//         Resources      # Data files such as images, sounds, translations and nib files
//             en.lproj        # Folder containing english translation strings/data
//         Frameworks     # A directory containing private frameworks (shared libraries)
//         PlugIns        # A directory containing Plugins
//         ...            # Any other optional files the developer wants to place here
//
// See https://developer.apple.com/go/?id=bundle-structure for a full
// explanation.
//
// Currently, cargo-bundle does not support Frameworks, nor does it support placing arbitrary
// files into the `Contents` directory of the bundle.

use super::common::{self, read_file};
use crate::Settings;
use anyhow::Context;
use image::imageops::FilterType::Lanczos3;
use image::{self, GenericImageView};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{Options, Tree};
use std::cmp::min;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufWriter, Cursor};
use std::path::{Path, PathBuf};

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let base_dir = settings.project_out_directory().join("bundle/osx");
    let path = bundle_project_at(settings, &base_dir)?;
    Ok(vec![path])
}

pub fn bundle_project_at(settings: &Settings, output_dir: &Path) -> crate::Result<PathBuf> {
    let app_bundle_name = format!("{}.app", settings.bundle_name());
    common::print_bundling(&app_bundle_name)?;
    let app_bundle_path = output_dir.join(&app_bundle_name);
    if app_bundle_path.exists() {
        fs::remove_dir_all(&app_bundle_path)
            .with_context(|| format!("Failed to remove old {app_bundle_name}"))?;
    }
    let bundle_directory = app_bundle_path.join("Contents");
    fs::create_dir_all(&bundle_directory)
        .with_context(|| format!("Failed to create bundle directory at {bundle_directory:?}"))?;

    let resources_dir = bundle_directory.join("Resources");

    let bundle_icon_file: Option<PathBuf> = {
        create_icns_file(&resources_dir, settings).with_context(|| "Failed to create app icon")?
    };

    create_info_plist(&bundle_directory, bundle_icon_file, settings)
        .with_context(|| "Failed to create Info.plist")?;

    let copied = copy_frameworks_to_bundle(&bundle_directory, settings)
        .with_context(|| "Failed to bundle frameworks")?;

    copy_plugins_to_bundle(&bundle_directory, settings)
        .with_context(|| "Failed to bundle plugins")?;

    for src in settings.resource_files() {
        let src = src?;
        let dest = resources_dir.join(common::resource_relpath(&src));
        common::copy_file(&src, &dest)
            .with_context(|| format!("Failed to copy resource file {src:?}"))?;
    }

    copy_binary_to_bundle(&bundle_directory, settings)
        .with_context(|| format!("Failed to copy binary from {:?}", settings.binary_path()))?;

    write_localizations(&resources_dir, settings)
        .with_context(|| "Failed to write localisation files")?;

    if copied > 0 {
        add_rpath(&bundle_directory, settings)?;
    }

    Ok(app_bundle_path)
}

#[allow(dead_code)]
#[derive(Debug, Default)]
struct DylibInfo {
    dylibs: Vec<PathBuf>,
    rpaths: Vec<PathBuf>,
}

impl DylibInfo {
    /// Inspect a Mach-O binary's load commands for linked dylibs and rpaths.
    fn inspect(dylib_path: &Path) -> crate::Result<Self> {
        use goblin::mach::{Mach, SingleArch};

        let bytes = std::fs::read(dylib_path)
            .with_context(|| format!("Failed to read {}", dylib_path.display()))?;

        let mut info = Self::default();
        match Mach::parse(&bytes)? {
            Mach::Binary(macho) => info.absorb(&macho),
            Mach::Fat(fat) => {
                for architecture in fat.into_iter() {
                    if let SingleArch::MachO(macho) = architecture? {
                        info.absorb(&macho);
                    }
                }
            }
        }
        Ok(info)
    }

    /// Collect the linked dylibs and rpaths from one Mach-O architecture slice.
    fn absorb(&mut self, macho: &goblin::mach::MachO) {
        self.dylibs.extend(macho.libs.iter().map(PathBuf::from));
        self.rpaths.extend(macho.rpaths.iter().map(PathBuf::from));
    }

    fn has_rpath<T: AsRef<Path>>(&self, path: T) -> bool {
        self.rpaths.iter().any(|s| s.as_path() == path.as_ref())
    }
}

fn copy_binary_to_bundle(bundle_directory: &Path, settings: &Settings) -> crate::Result<()> {
    let dest_dir = bundle_directory.join("MacOS");
    common::copy_file(
        settings.binary_path(),
        &dest_dir.join(settings.binary_name()),
    )
}
trait PlistEntryFormatter {
    fn format_plist_entry(&self) -> String;
}

impl<T: AsRef<str>> PlistEntryFormatter for T {
    fn format_plist_entry(&self) -> String {
        let input = self.as_ref();
        input.replace("&", "&amp;")
        // add other necessary modifications here...
    }
}

const FRAMEWORKS_RPATH: &str = "@executable_path/../Frameworks";

fn add_rpath(bundle_directory: &Path, settings: &Settings) -> crate::Result<()> {
    let bin = bundle_directory.join("MacOS").join(settings.binary_name());

    let dyinfo = DylibInfo::inspect(&bin)?;

    if dyinfo.has_rpath(FRAMEWORKS_RPATH) {
        //rpath already in dylib
        return Ok(());
    }

    if !std::process::Command::new("install_name_tool")
        .arg("-add_rpath")
        .arg(FRAMEWORKS_RPATH)
        .arg(bin)
        .status()?
        .success()
    {
        anyhow::bail!("failed to execute install_name_tool");
    }

    Ok(())
}

fn create_info_plist(
    bundle_dir: &Path,
    bundle_icon_file: Option<PathBuf>,
    settings: &Settings,
) -> crate::Result<()> {
    let build_number = chrono::Utc::now().format("%Y%m%d.%H%M%S");
    let file = &mut common::create_file(&bundle_dir.join("Info.plist"))?;
    write!(
        file,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
            <!DOCTYPE plist PUBLIC \"-//Apple Computer//DTD PLIST 1.0//EN\" \
            \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
            <plist version=\"1.0\">\n\
            <dict>\n"
    )?;
    write!(
        file,
        "  <key>CFBundleDevelopmentRegion</key>\n  \
            <string>English</string>\n"
    )?;
    write!(
        file,
        "  <key>CFBundleDisplayName</key>\n  <string>{}</string>\n",
        settings.bundle_name().format_plist_entry()
    )?;
    write!(
        file,
        "  <key>CFBundleExecutable</key>\n  <string>{}</string>\n",
        settings.binary_name()
    )?;
    if let Some(path) = bundle_icon_file {
        write!(
            file,
            "  <key>CFBundleIconFile</key>\n  <string>{}</string>\n",
            path.file_name().unwrap().to_string_lossy()
        )?;
    }
    write!(
        file,
        "  <key>CFBundleIdentifier</key>\n  <string>{}</string>\n",
        settings.bundle_identifier()
    )?;
    write!(
        file,
        "  <key>CFBundleInfoDictionaryVersion</key>\n  \
            <string>6.0</string>\n"
    )?;
    write!(
        file,
        "  <key>CFBundleName</key>\n  <string>{}</string>\n",
        settings.bundle_name().format_plist_entry()
    )?;
    write!(
        file,
        "  <key>CFBundlePackageType</key>\n  <string>APPL</string>\n"
    )?;
    write!(
        file,
        "  <key>CFBundleShortVersionString</key>\n  <string>{}</string>\n",
        settings.version_string()
    )?;
    if !settings.osx_url_schemes().is_empty() {
        write!(
            file,
            "  <key>CFBundleURLTypes</key>\n  \
               <array>\n    \
                   <dict>\n      \
                       <key>CFBundleURLName</key>\n      \
                       <string>{}</string>\n      \
                       <key>CFBundleTypeRole</key>\n      \
                       <string>Viewer</string>\n      \
                       <key>CFBundleURLSchemes</key>\n      \
                       <array>\n",
            settings.bundle_name().format_plist_entry()
        )?;
        for scheme in settings.osx_url_schemes() {
            writeln!(
                file,
                "        <string>{}</string>",
                scheme.format_plist_entry()
            )?;
        }
        write!(
            file,
            "      </array>\n    \
                </dict>\n  \
             </array>\n"
        )?;
    }
    write!(
        file,
        "  <key>CFBundleVersion</key>\n  <string>{build_number}</string>\n"
    )?;
    write!(file, "  <key>CSResourcesFileMapped</key>\n  <true/>\n")?;
    if let Some(category) = settings.app_category() {
        write!(
            file,
            "  <key>LSApplicationCategoryType</key>\n  \
                <string>{}</string>\n",
            category
                .osx_application_category_type()
                .format_plist_entry()
        )?;
    }
    if let Some(version) = settings.osx_minimum_system_version() {
        write!(
            file,
            "  <key>LSMinimumSystemVersion</key>\n  \
                <string>{version}</string>\n"
        )?;
    }
    write!(file, "  <key>LSRequiresCarbon</key>\n  <true/>\n")?;
    write!(file, "  <key>NSHighResolutionCapable</key>\n  <true/>\n")?;
    if let Some(copyright) = settings.copyright_string() {
        write!(
            file,
            "  <key>NSHumanReadableCopyright</key>\n  \
                <string>{}</string>\n",
            copyright.format_plist_entry()
        )?;
    }
    for plist in settings.osx_info_plist_exts() {
        let plist = plist?;
        let contents = read_file(&plist)?;
        write!(file, "{:}", contents.format_plist_entry())?
    }
    write!(file, "</dict>\n</plist>\n")?;
    file.flush()?;
    Ok(())
}

fn copy_framework_from(dest_dir: &Path, framework: &str, src_dir: &Path) -> crate::Result<bool> {
    let src_name = format!("{framework}.framework");
    let src_path = src_dir.join(&src_name);
    if src_path.exists() {
        common::copy_dir(&src_path, &dest_dir.join(&src_name))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn copy_frameworks_to_bundle(bundle_directory: &Path, settings: &Settings) -> crate::Result<i32> {
    let frameworks = settings.osx_frameworks();
    if frameworks.is_empty() {
        return Ok(0);
    }
    let mut copied = 0;
    let dest_dir = bundle_directory.join("Frameworks");
    fs::create_dir_all(bundle_directory)
        .with_context(|| format!("Failed to create Frameworks directory at {dest_dir:?}"))?;
    for framework in frameworks.iter() {
        if framework.ends_with(".framework") {
            let src_path = PathBuf::from(framework);
            let src_name = src_path.file_name().unwrap();
            common::copy_dir(&src_path, &dest_dir.join(src_name))?;
            copied += 1;
            continue;
        } else if framework.ends_with(".dylib") {
            let src_path = PathBuf::from(framework);
            let src_name = src_path.file_name().unwrap();
            common::copy_file(&src_path, &dest_dir.join(src_name))?;
            copied += 1;
            continue;
        } else if framework.contains('/') {
            anyhow::bail!(
                "Framework path should have .framework extension: {}",
                framework
            );
        }
        if let Some(home_dir) = dirs::home_dir()
            && copy_framework_from(&dest_dir, framework, &home_dir.join("Library/Frameworks/"))?
        {
            copied += 1;
            continue;
        }
        if copy_framework_from(&dest_dir, framework, &PathBuf::from("/Library/Frameworks/"))?
            || copy_framework_from(
                &dest_dir,
                framework,
                &PathBuf::from("/Network/Library/Frameworks/"),
            )?
            || copy_framework_from(
                &dest_dir,
                framework,
                &PathBuf::from("/System/Library/Frameworks/"),
            )?
        {
            copied += 1;
            continue;
        }
        anyhow::bail!("Could not locate {}.framework", framework);
    }
    Ok(copied)
}

fn copy_plugins_to_bundle(bundle_directory: &Path, settings: &Settings) -> crate::Result<()> {
    let plugins = settings.osx_plugins();
    if plugins.is_empty() {
        return Ok(());
    }
    let dest_dir = bundle_directory.join("PlugIns");
    fs::create_dir_all(bundle_directory)
        .with_context(|| format!("Failed to create PlugIns directory at {dest_dir:?}"))?;
    for plugin in plugins.iter() {
        let src_path = PathBuf::from(plugin);
        let src_name = src_path.file_name().unwrap();
        common::copy_dir(&src_path, &dest_dir.join(src_name))?;
    }
    Ok(())
}

/// Given a list of icon files, try to produce an ICNS file in the resources
/// directory and return the path to it.  Returns `Ok(None)` if no usable icons
/// were provided.
fn create_icns_file(
    resources_dir: &PathBuf,
    settings: &Settings,
) -> crate::Result<Option<PathBuf>> {
    if settings.icon_files().count() == 0 {
        return Ok(None);
    }

    // If one of the icon files is an SVG, convert it via resvg + icns crate.
    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("svg")) {
            fs::create_dir_all(resources_dir)?;
            let mut dest_path = resources_dir.clone();
            dest_path.push(settings.bundle_name());
            dest_path.set_extension("icns");
            create_icns_from_svg(&icon_path, resources_dir, &dest_path)
                .with_context(|| format!("Failed to convert SVG icon {icon_path:?} to ICNS"))?;
            return Ok(Some(dest_path));
        }
    }

    // If one of the icon files is already an ICNS file, just use that.
    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("icns")) {
            let mut dest_path = resources_dir.to_path_buf();
            dest_path.push(icon_path.file_name().unwrap());
            common::copy_file(&icon_path, &dest_path)?;
            return Ok(Some(dest_path));
        }
    }

    // Otherwise, read available images and pack them into a new ICNS file.
    let mut family = icns::IconFamily::new();

    fn add_icon_to_family(
        icon: image::DynamicImage,
        density: u32,
        family: &mut icns::IconFamily,
    ) -> io::Result<()> {
        // Try to add this image to the icon family.  Ignore images whose sizes
        // don't map to any ICNS icon type; print warnings and skip images that
        // fail to encode.
        match icns::IconType::from_pixel_size_and_density(icon.width(), icon.height(), density) {
            Some(icon_type) => {
                if !family.has_icon_with_type(icon_type) {
                    let icon = make_icns_image(icon)?;
                    family.add_icon_with_type(&icon, icon_type)?;
                }
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "No matching IconType",
            )),
        }
    }

    let mut images_to_resize: Vec<(image::DynamicImage, u32, u32)> = vec![];
    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("svg")) {
            continue;
        }
        let icon = image::open(&icon_path)?;
        let density = if common::is_retina(&icon_path) { 2 } else { 1 };
        let (w, h) = icon.dimensions();
        let orig_size = min(w, h);
        let next_size_down = 2f32.powf((orig_size as f32).log2().floor()) as u32;
        if orig_size > next_size_down {
            images_to_resize.push((icon, next_size_down, density));
        } else {
            add_icon_to_family(icon, density, &mut family)?;
        }
    }

    for (icon, next_size_down, density) in images_to_resize {
        let icon = icon.resize_exact(next_size_down, next_size_down, Lanczos3);
        add_icon_to_family(icon, density, &mut family)?;
    }

    if !family.is_empty() {
        fs::create_dir_all(resources_dir)?;
        let mut dest_path = resources_dir.clone();
        dest_path.push(settings.bundle_name());
        dest_path.set_extension("icns");
        let icns_file = BufWriter::new(File::create(&dest_path)?);
        family.write(icns_file)?;
        return Ok(Some(dest_path));
    }

    anyhow::bail!("No usable icon files found.");
}

/// Renders an SVG icon at standard macOS iconset sizes and produces an ICNS file at `dest_path`.
fn create_icns_from_svg(
    svg_path: &Path,
    resources_dir: &Path,
    dest_path: &Path,
) -> crate::Result<()> {
    let svg_data = fs::read_to_string(svg_path)
        .with_context(|| format!("Failed to read SVG file {svg_path:?}"))?;

    let opt = Options::default();
    let tree =
        Tree::from_data(svg_data.as_bytes(), &opt).with_context(|| "Failed to parse SVG data")?;

    let mut family = icns::IconFamily::new();

    // Standard macOS iconset sizes: (logical_size, scale_factor)
    let sizes: &[(u32, u32)] = &[
        (16, 1),
        (16, 2),
        (32, 1),
        (32, 2),
        (128, 1),
        (128, 2),
        (256, 1),
        (256, 2),
        (512, 1),
        (512, 2),
    ];

    for &(size, scale) in sizes {
        let pixel_size = size * scale;
        let mut pixmap = Pixmap::new(pixel_size, pixel_size)
            .with_context(|| format!("Failed to create {pixel_size}x{pixel_size} pixmap"))?;
        resvg::render(
            &tree,
            Transform::from_scale(
                pixel_size as f32 / tree.size().width(),
                pixel_size as f32 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );

        let png_data = pixmap
            .encode_png()
            .with_context(|| format!("Failed to encode {pixel_size}x{pixel_size} pixmap to PNG"))?;

        let icon_image = icns::Image::read_png(Cursor::new(&png_data))
            .with_context(|| format!("Failed to read PNG for {pixel_size}x{pixel_size} icon"))?;

        let icon_type = icns::IconType::from_pixel_size_and_density(pixel_size, pixel_size, scale)
            .with_context(|| format!("No ICNS icon type for size {size} and scale {scale}"))?;

        family
            .add_icon_with_type(&icon_image, icon_type)
            .with_context(|| format!("Failed to add {size}x{size}@{scale}x icon to ICNS family"))?;
    }

    fs::create_dir_all(resources_dir)?;
    let icns_file = BufWriter::new(File::create(dest_path)?);
    family
        .write(icns_file)
        .with_context(|| format!("Failed to write ICNS file to {dest_path:?}"))?;

    Ok(())
}

/// Writes `*.lproj/InfoPlist.strings` localisation files into `resources_dir`
/// for every locale present in the settings' `osx_localizations` map.
fn write_localizations(resources_dir: &Path, settings: &Settings) -> crate::Result<()> {
    let Some(localizations) = settings.osx_localizations() else {
        return Ok(());
    };

    for (locale, strings) in localizations {
        let lproj_dir = resources_dir.join(locale).with_extension("lproj");
        fs::create_dir_all(&lproj_dir)
            .with_context(|| format!("Failed to create {lproj_dir:?}"))?;

        let strings_path = lproj_dir.join("InfoPlist.strings");
        let file = &mut common::create_file(&strings_path)?;
        for (key, value) in strings {
            // Escape embedded double-quotes in the value.
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            writeln!(file, "{key} = \"{escaped}\";")?;
        }
        file.flush()?;
    }

    Ok(())
}

/// Converts an image::DynamicImage into an icns::Image.
fn make_icns_image(img: image::DynamicImage) -> io::Result<icns::Image> {
    let pixel_format = match img.color() {
        image::ColorType::Rgba8 => icns::PixelFormat::RGBA,
        image::ColorType::Rgb8 => icns::PixelFormat::RGB,
        image::ColorType::La8 => icns::PixelFormat::GrayAlpha,
        image::ColorType::L8 => icns::PixelFormat::Gray,
        _ => {
            let msg = format!("unsupported ColorType: {:?}", img.color());
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
        }
    };
    icns::Image::from_data(pixel_format, img.width(), img.height(), img.into_bytes())
}
