// Finder window decoration for DMG volumes.
//
// When a background SVG is configured, the mounted volume receives:
//
//   .background/background.tiff  # the SVG rendered at 2x (144 dpi)
//   .DS_Store                    # window bounds, view options and icon spots
//
// The SVG must contain elements with ids `app` and `applications`; the center
// of each becomes the icon position of the application bundle and of the
// /Applications symlink respectively. This mirrors what tools like dmgbuild
// produce, but is generated entirely in Rust via the vendored `ds` crate.

use crate::Settings;
use anyhow::Context;
use ds_parser::{FieldKey, Record, Value, write as write_dsstore};
use plist::Dictionary;
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;
use tiff::encoder::{Rational, TiffEncoder, colortype};
use tiff::tags::ResolutionUnit;

/// Icon size used in the Finder window, matching common DMG installers.
const ICON_SIZE: u64 = 48;

/// Screen position of the top-left corner of the Finder window.
const WINDOW_POSITION: (u32, u32) = (100, 100);

/// Decorate the mounted DMG volume with a background image and icon layout.
/// Does nothing unless a background SVG is configured.
pub fn decorate_volume(
    settings: &Settings,
    mount_point: &Path,
    app_bundle_name: &OsStr,
) -> crate::Result<()> {
    let app_bundle_name = app_bundle_name
        .to_str()
        .with_context(|| "Application bundle name is not valid UTF-8")?;

    let Some(background_path) = settings.osx_dmg_background() else {
        return Ok(());
    };

    let svg_data = fs::read(background_path)
        .with_context(|| format!("Failed to read DMG background {background_path:?}"))?;
    let tree = Tree::from_data(&svg_data, &Options::default())
        .with_context(|| format!("Failed to parse DMG background SVG {background_path:?}"))?;

    let app_position = center_of_element(&tree, "app")?;
    let applications_position = center_of_element(&tree, "applications")?;

    let background_file = mount_point.join(".background").join("background.tiff");
    render_background(&tree, &background_file)?;

    let window_size = (tree.size().width() as u32, tree.size().height() as u32);
    let background_alias = AliasRecord::for_file(&background_file, mount_point)?.serialize();

    let records = vec![
        Record {
            name: ".".into(),
            field: FieldKey::vSrn,
            value: Value::Long(1),
        },
        Record {
            name: ".".into(),
            field: FieldKey::bwsp,
            value: Value::Blob(window_settings_plist(WINDOW_POSITION, window_size)?),
        },
        Record {
            name: ".".into(),
            field: FieldKey::icvp,
            value: Value::Blob(icon_view_plist(background_alias)?),
        },
        icon_location_record("Applications", applications_position),
        icon_location_record(app_bundle_name, app_position),
    ];

    let ds_store = write_dsstore(&records).with_context(|| "Failed to build .DS_Store")?;
    fs::write(mount_point.join(".DS_Store"), ds_store)
        .with_context(|| "Failed to write .DS_Store to DMG volume")?;
    Ok(())
}

/// Render the SVG at double resolution into a 144 dpi TIFF, so the
/// background appears crisp on Retina displays at its nominal size.
fn render_background(tree: &Tree, output_path: &Path) -> crate::Result<()> {
    let mut pixmap = Pixmap::new(
        tree.size().width() as u32 * 2,
        tree.size().height() as u32 * 2,
    )
    .with_context(|| "Failed to allocate pixmap for DMG background")?;
    resvg::render(
        tree,
        Transform::from_scale(
            pixmap.width() as f32 / tree.size().width(),
            pixmap.height() as f32 / tree.size().height(),
        ),
        &mut pixmap.as_mut(),
    );

    fs::create_dir_all(output_path.parent().unwrap())
        .with_context(|| "Failed to create .background directory in DMG volume")?;
    let mut file = fs::File::create(output_path)
        .with_context(|| format!("Failed to create {output_path:?}"))?;
    let mut encoder = TiffEncoder::new(&mut file)?;
    let mut tiff_image = encoder.new_image::<colortype::RGBA8>(pixmap.width(), pixmap.height())?;
    tiff_image.resolution(ResolutionUnit::Inch, Rational { n: 144, d: 1 });
    tiff_image
        .write_data(pixmap.data())
        .with_context(|| "Failed to encode DMG background TIFF")?;
    Ok(())
}

/// The center of the SVG element carrying the given id, in SVG coordinates.
fn center_of_element(tree: &Tree, id: &str) -> crate::Result<(u32, u32)> {
    let node = tree.node_by_id(id).with_context(|| {
        format!("DMG background SVG does not contain an element with id \"{id}\"")
    })?;
    let bounds = node.abs_stroke_bounding_box();
    Ok((
        (bounds.x() + bounds.width() / 2.0) as u32,
        (bounds.y() + bounds.height() / 2.0) as u32,
    ))
}

/// An `Iloc` record placing a file's icon at the given position.
fn icon_location_record(file_name: &str, position: (u32, u32)) -> Record {
    let mut blob = Vec::with_capacity(12);
    blob.extend_from_slice(&position.0.to_be_bytes());
    blob.extend_from_slice(&position.1.to_be_bytes());
    blob.extend_from_slice(b"\xFF\xFF\xFF\x00");
    Record {
        name: file_name.into(),
        field: FieldKey::Iloc,
        value: Value::Blob(blob),
    }
}

/// The `bwsp` binary plist describing the Finder window bounds and chrome.
fn window_settings_plist(position: (u32, u32), size: (u32, u32)) -> crate::Result<Vec<u8>> {
    let mut dictionary = Dictionary::new();
    dictionary.insert("ContainerShowSidebar".into(), plist::Value::Boolean(true));
    dictionary.insert("ShowPathbar".into(), plist::Value::Boolean(false));
    dictionary.insert("ShowSidebar".into(), plist::Value::Boolean(true));
    dictionary.insert("ShowStatusBar".into(), plist::Value::Boolean(false));
    dictionary.insert("ShowTabView".into(), plist::Value::Boolean(false));
    dictionary.insert("ShowToolbar".into(), plist::Value::Boolean(false));
    dictionary.insert("SidebarWidth".into(), plist::Value::Integer(0.into()));
    dictionary.insert(
        "WindowBounds".into(),
        plist::Value::String(format!(
            "{{{{{},{}}},{{{},{}}}}}",
            position.0, position.1, size.0, size.1
        )),
    );
    encode_binary_plist(dictionary)
}

/// The `icvp` binary plist selecting icon view with the background image.
fn icon_view_plist(background_alias: Vec<u8>) -> crate::Result<Vec<u8>> {
    let mut dictionary = Dictionary::new();
    dictionary.insert("backgroundType".into(), plist::Value::Integer(2.into()));
    dictionary.insert("backgroundColorRed".into(), plist::Value::Real(1.0));
    dictionary.insert("backgroundColorGreen".into(), plist::Value::Real(1.0));
    dictionary.insert("backgroundColorBlue".into(), plist::Value::Real(1.0));
    dictionary.insert(
        "backgroundImageAlias".into(),
        plist::Value::Data(background_alias),
    );
    dictionary.insert("showIconPreview".into(), plist::Value::Boolean(true));
    dictionary.insert("showItemInfo".into(), plist::Value::Boolean(false));
    dictionary.insert("textSize".into(), plist::Value::Integer(12.into()));
    dictionary.insert("iconSize".into(), plist::Value::Integer(ICON_SIZE.into()));
    dictionary.insert("viewOptionsVersion".into(), plist::Value::Integer(1.into()));
    dictionary.insert("gridSpacing".into(), plist::Value::Real(100.0));
    dictionary.insert("gridOffsetX".into(), plist::Value::Real(0.0));
    dictionary.insert("gridOffsetY".into(), plist::Value::Real(0.0));
    dictionary.insert("labelOnBottom".into(), plist::Value::Boolean(true));
    dictionary.insert("arrangeBy".into(), plist::Value::String("none".into()));
    encode_binary_plist(dictionary)
}

fn encode_binary_plist(dictionary: Dictionary) -> crate::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    plist::to_writer_binary(&mut buffer, &dictionary)
        .with_context(|| "Failed to encode binary plist")?;
    Ok(buffer)
}

/// Seconds between the HFS+ epoch (1904-01-01) and the Unix epoch.
const HFS_EPOCH_OFFSET: u64 = 2_082_844_800;

/// The fixed part of a version-2 alias record is 150 bytes long.
const ALIAS_BASE_LENGTH: u16 = 150;

/// A classic macOS Alias record pointing at a file on a mounted volume.
/// Finder requires the `icvp` background image to be referenced this way.
struct AliasRecord {
    volume_name: String,
    volume_created: SystemTime,
    parent_inode: u32,
    parent_name: String,
    target_inode: u32,
    target_name: String,
    target_created: SystemTime,
    path_from_volume: String,
    volume_path: String,
}

impl AliasRecord {
    fn for_file(path: &Path, mount_point: &Path) -> crate::Result<Self> {
        let parent = path
            .parent()
            .with_context(|| format!("{path:?} has no parent directory"))?;
        let target_metadata = fs::metadata(path)?;
        let parent_metadata = fs::metadata(parent)?;
        let volume_metadata = fs::metadata(mount_point)?;
        let file_name = |p: &Path| -> crate::Result<String> {
            Ok(p.file_name()
                .with_context(|| format!("{p:?} has no file name"))?
                .to_string_lossy()
                .into_owned())
        };
        Ok(AliasRecord {
            volume_name: file_name(mount_point)?,
            volume_created: volume_metadata.created()?,
            parent_inode: parent_metadata.ino() as u32,
            parent_name: file_name(parent)?,
            target_inode: target_metadata.ino() as u32,
            target_name: file_name(path)?,
            target_created: target_metadata.created()?,
            path_from_volume: format!("/{}", path.strip_prefix(mount_point)?.to_string_lossy()),
            volume_path: mount_point.to_string_lossy().into_owned(),
        })
    }

    fn serialize(&self) -> Vec<u8> {
        // Extras are (type, data) pairs appended after the fixed fields.
        let extras: Vec<(i16, Vec<u8>)> = vec![
            (0, self.parent_name.as_bytes().to_vec()),
            (1, self.parent_inode.to_be_bytes().to_vec()),
            (14, counted_utf16(&self.target_name)),
            (15, counted_utf16(&self.volume_name)),
            (18, self.path_from_volume.as_bytes().to_vec()),
            (19, self.volume_path.as_bytes().to_vec()),
        ];

        let extras_length: usize = extras
            .iter()
            .map(|(_, data)| 4 + data.len() + data.len() % 2)
            .sum();
        let total_length = ALIAS_BASE_LENGTH as usize + extras_length + 4;

        let mut buffer = Vec::with_capacity(total_length);
        buffer.extend_from_slice(&0u32.to_be_bytes()); // application signature
        buffer.extend_from_slice(&(total_length as u16).to_be_bytes());
        buffer.extend_from_slice(&2u16.to_be_bytes()); // record version
        buffer.extend_from_slice(&0u16.to_be_bytes()); // target type: file

        buffer.push(self.volume_name.len() as u8);
        push_padded(&mut buffer, &self.volume_name, 27);
        buffer.extend_from_slice(&hfs_timestamp(self.volume_created).to_be_bytes());
        buffer.extend_from_slice(b"H+"); // volume signature
        buffer.extend_from_slice(&5u16.to_be_bytes()); // volume type: other

        buffer.extend_from_slice(&self.parent_inode.to_be_bytes());
        buffer.push(self.target_name.len() as u8);
        push_padded(&mut buffer, &self.target_name, 63);
        buffer.extend_from_slice(&self.target_inode.to_be_bytes());
        buffer.extend_from_slice(&hfs_timestamp(self.target_created).to_be_bytes());

        buffer.extend_from_slice(&[0u8; 8]); // file type and creator codes
        buffer.extend_from_slice(&(-1i16).to_be_bytes()); // alias-to-alias levels
        buffer.extend_from_slice(&(-1i16).to_be_bytes());
        buffer.extend_from_slice(&0x0000_0D02u32.to_be_bytes()); // volume attributes
        buffer.extend_from_slice(&0u16.to_be_bytes()); // filesystem id
        buffer.extend_from_slice(&[0u8; 10]); // reserved

        for (type_code, data) in &extras {
            buffer.extend_from_slice(&type_code.to_be_bytes());
            buffer.extend_from_slice(&(data.len() as u16).to_be_bytes());
            buffer.extend_from_slice(data);
            if data.len() % 2 == 1 {
                buffer.push(0);
            }
        }
        buffer.extend_from_slice(&(-1i16).to_be_bytes()); // end-of-extras marker
        buffer.extend_from_slice(&0u16.to_be_bytes());
        buffer
    }
}

/// A UTF-16BE string prefixed with its length in characters.
fn counted_utf16(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut data = (units.len() as u16).to_be_bytes().to_vec();
    for unit in units {
        data.extend_from_slice(&unit.to_be_bytes());
    }
    data
}

/// A string in a fixed-size field, zero-padded to `length` bytes.
fn push_padded(buffer: &mut Vec<u8>, text: &str, length: usize) {
    let bytes = text.as_bytes();
    buffer.extend_from_slice(bytes);
    buffer.resize(buffer.len() + length.saturating_sub(bytes.len()), 0);
}

fn hfs_timestamp(time: SystemTime) -> u32 {
    let unix_seconds = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (unix_seconds + HFS_EPOCH_OFFSET) as u32
}
