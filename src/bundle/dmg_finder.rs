use crate::Settings;
use anyhow::Context;
use dmg_layout::{
    AliasHeader, AliasKind, AliasMetadata, AliasRecord, AliasTarget, AliasVolume, CatalogNodeId,
    DiskType, FilesystemSignature, HfsTimestampSeconds, VolumeMountPath,
};
use ds_parser::{FieldKey, IconLocation, PropertyList, Record, write};
use plist::{Dictionary, Value};
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use tiff::encoder::{Rational, TiffEncoder, colortype};
use tiff::tags::ResolutionUnit;

const ICON_SIZE: u64 = 48;

const WINDOW_POSITION: (u32, u32) = (100, 100);

/// Filesystem facts needed to describe one component of a Finder Alias record.
struct AliasPath<'a> {
    path: &'a Path,
    name: String,
    metadata: fs::Metadata,
}

impl<'a> AliasPath<'a> {
    fn load(path: &'a Path, description: &str) -> crate::Result<Self> {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .with_context(|| {
                format!(
                    "{description} has no filename component: {}",
                    path.display()
                )
            })?;
        let metadata = fs::metadata(path).with_context(|| {
            format!("Failed to read {description} metadata: {}", path.display())
        })?;
        Ok(Self {
            path,
            name,
            metadata,
        })
    }

    fn hfs_creation_time(&self) -> crate::Result<HfsTimestampSeconds> {
        const HFS_EPOCH_OFFSET_SECONDS: u64 = 2_082_844_800;
        let created = self.metadata.created().with_context(|| {
            format!(
                "Filesystem does not expose creation time for {}",
                self.path.display()
            )
        })?;
        let unix_seconds = created
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(HfsTimestampSeconds::from(
            (unix_seconds + HFS_EPOCH_OFFSET_SECONDS) as u32,
        ))
    }

    fn catalog_node_id(&self) -> u32 {
        file_id(&self.metadata)
    }
}

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
    let background_alias = background_alias_record(&background_file, mount_point)
        .with_context(|| "Failed to build alias record for DMG background")?
        .to_bytes()
        .with_context(|| "Failed to serialise DMG background alias record")?;

    let records = vec![
        Record::new(".", FieldKey::vSrn, 1u32),
        Record::new(
            ".",
            FieldKey::bwsp,
            window_settings_plist(WINDOW_POSITION, window_size),
        ),
        Record::new(".", FieldKey::icvp, icon_view_plist(background_alias)),
        icon_location_record("Applications", applications_position),
        icon_location_record(app_bundle_name, app_position),
    ];

    let ds_store = write(&records).with_context(|| "Failed to build .DS_Store")?;
    fs::write(mount_point.join(".DS_Store"), ds_store)
        .with_context(|| "Failed to write .DS_Store to DMG volume")?;
    Ok(())
}

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

fn icon_location_record(file_name: &str, position: (u32, u32)) -> Record {
    Record::new(
        file_name,
        FieldKey::Iloc,
        IconLocation::new(position.0, position.1),
    )
}

fn window_settings_plist(position: (u32, u32), size: (u32, u32)) -> PropertyList {
    let mut dictionary = Dictionary::new();
    dictionary.insert("ContainerShowSidebar".into(), Value::Boolean(true));
    dictionary.insert("ShowPathbar".into(), Value::Boolean(false));
    dictionary.insert("ShowSidebar".into(), Value::Boolean(true));
    dictionary.insert("ShowStatusBar".into(), Value::Boolean(false));
    dictionary.insert("ShowTabView".into(), Value::Boolean(false));
    dictionary.insert("ShowToolbar".into(), Value::Boolean(false));
    dictionary.insert("SidebarWidth".into(), Value::Integer(0.into()));
    dictionary.insert(
        "WindowBounds".into(),
        Value::String(format!(
            "{{{{{},{}}},{{{},{}}}}}",
            position.0, position.1, size.0, size.1
        )),
    );
    PropertyList::Dictionary(dictionary)
}

fn icon_view_plist(background_alias: Vec<u8>) -> PropertyList {
    let mut dictionary = Dictionary::new();
    dictionary.insert("backgroundType".into(), Value::Integer(2.into()));
    dictionary.insert("backgroundColorRed".into(), Value::Real(1.0));
    dictionary.insert("backgroundColorGreen".into(), Value::Real(1.0));
    dictionary.insert("backgroundColorBlue".into(), Value::Real(1.0));
    dictionary.insert("backgroundImageAlias".into(), Value::Data(background_alias));
    dictionary.insert("showIconPreview".into(), Value::Boolean(true));
    dictionary.insert("showItemInfo".into(), Value::Boolean(false));
    dictionary.insert("textSize".into(), Value::Integer(12.into()));
    dictionary.insert("iconSize".into(), Value::Integer(ICON_SIZE.into()));
    dictionary.insert("viewOptionsVersion".into(), Value::Integer(1.into()));
    dictionary.insert("gridSpacing".into(), Value::Real(100.0));
    dictionary.insert("gridOffsetX".into(), Value::Real(0.0));
    dictionary.insert("gridOffsetY".into(), Value::Real(0.0));
    dictionary.insert("labelOnBottom".into(), Value::Boolean(true));
    dictionary.insert("arrangeBy".into(), Value::String("none".into()));
    PropertyList::Dictionary(dictionary)
}

fn background_alias_record(target_path: &Path, volume_mount: &Path) -> crate::Result<AliasRecord> {
    let parent_path = target_path.parent().with_context(|| {
        format!(
            "DMG background path has no parent: {}",
            target_path.display()
        )
    })?;
    let relative_path = target_path.strip_prefix(volume_mount).with_context(|| {
        format!(
            "DMG background {} is outside mounted volume {}",
            target_path.display(),
            volume_mount.display()
        )
    })?;

    let volume_info = AliasPath::load(volume_mount, "DMG volume")?;
    let parent_info = AliasPath::load(parent_path, "DMG background directory")?;
    let target_info = AliasPath::load(target_path, "DMG background")?;
    let parent_catalog_node_id = parent_info.catalog_node_id();
    let target_catalog_node_id = target_info.catalog_node_id();

    let volume = AliasVolume::new(
        volume_info.name.as_bytes(),
        volume_info.hfs_creation_time()?,
        FilesystemSignature::HIERARCHICAL_FILE_SYSTEM_PLUS,
        DiskType::Ejectable,
    )?;
    let target = AliasTarget::new(
        AliasKind::File,
        CatalogNodeId(parent_catalog_node_id),
        target_info.name.as_bytes(),
        CatalogNodeId(target_catalog_node_id),
        target_info.hfs_creation_time()?,
    )?;
    let target_path = format!("/{}", relative_path.to_string_lossy());
    let volume_path = volume_mount.to_string_lossy();
    let metadata = AliasMetadata::builder()
        .header(AliasHeader::new(volume, target))
        .parent_folder_name(Some(parent_info.name.into_bytes()))
        .catalog_node_id_path(vec![parent_catalog_node_id])
        .unicode_target_name(Some(target_info.name))
        .unicode_volume_name(Some(volume_info.name))
        .target_path(Some(target_path.into_bytes()))
        .volume_path(Some(VolumeMountPath::new(volume_path.as_bytes())?))
        .build();
    Ok(AliasRecord::from_metadata(metadata)?)
}

fn file_id(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.ino() as u32
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0
    }
}
