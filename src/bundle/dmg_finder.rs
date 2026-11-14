use super::common;
use crate::Settings;
use anyhow::Context;
use dmg_layout::{
    AliasHeader, AliasKind, AliasMetadata, AliasRecord, AliasTarget, AliasVolume, CatalogNodeId,
    DiskType, FilesystemSignature, HfsTimestampSeconds, VolumeMountPath,
};
use ds_parser::{
    BrowserWindowSettings, FieldKey, IconLocation, IconViewProperties, PropertyList, Record,
    WindowBounds, write,
};
use fstool::fs::hfs_plus::HfsPlus;
use resvg::usvg::Tree;
use std::fs;
use std::path::Path;
use tiff::encoder::{Rational, TiffEncoder, colortype};
use tiff::tags::ResolutionUnit;

const ICON_SIZE: f64 = 48.0;
const WINDOW_POSITION: (u32, u32) = (100, 100);
const BACKGROUND_FOLDER: &str = ".background";
const BACKGROUND_FILE: &str = "background.tiff";

/// Where the Finder window and its icons go. It is derived from the SVG
/// background up front; the `.DS_Store` itself waits for the catalog IDs.
pub(super) struct Decoration {
    window_size: (u32, u32),
    app_position: (u32, u32),
    applications_position: (u32, u32),
}

/// Renders the configured background into `volume_tree` and measures its layout.
/// Returns `None` when no `dmg_background` is set.
pub(super) fn prepare(
    settings: &Settings,
    volume_tree: &Path,
) -> crate::Result<Option<Decoration>> {
    let Some(background_path) = settings.osx_dmg_background() else {
        return Ok(None);
    };
    let tree = common::load_svg(background_path)?;

    let decoration = Decoration {
        window_size: (tree.size().width() as u32, tree.size().height() as u32),
        app_position: center_of_element(&tree, "app")?,
        applications_position: center_of_element(&tree, "applications")?,
    };
    let directory = volume_tree.join(BACKGROUND_FOLDER);
    fs::create_dir_all(&directory).with_context(|| format!("Failed to create {directory:?}"))?;
    render_background(&tree, &directory.join(BACKGROUND_FILE))?;
    Ok(Some(decoration))
}

/// Builds the `.DS_Store` that lays out the Finder window. `volume` must
/// already contain the application and the rendered background.
pub(super) fn ds_store(
    application_name: &str,
    decoration: &Decoration,
    volume: &HfsPlus,
) -> crate::Result<Vec<u8>> {
    // HFS+ may store the name differently from how the host spelled it, and
    // Finder looks icons up by the stored spelling.
    let stored_name = volume.entry_info(&format!("/{application_name}"))?.name;
    let background_alias = background_alias(volume)?.to_bytes().map_err(alias_error)?;

    let records = vec![
        Record::new(".", FieldKey::vSrn, 1u32),
        Record::new(
            ".",
            FieldKey::bwsp,
            window_settings(WINDOW_POSITION, decoration.window_size),
        ),
        Record::new(".", FieldKey::icvp, icon_view(background_alias)),
        icon_location("Applications", decoration.applications_position),
        icon_location(&stored_name, decoration.app_position),
    ];
    write(&records).map_err(|error| anyhow::anyhow!("Failed to build .DS_Store: {error}"))
}

fn render_background(tree: &Tree, output_path: &Path) -> crate::Result<()> {
    // Rendered at twice the window size so it stays sharp on Retina displays.
    let pixmap = common::render_svg(
        tree,
        tree.size().width() as u32 * 2,
        tree.size().height() as u32 * 2,
    )?;

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

fn icon_location(file_name: &str, position: (u32, u32)) -> Record {
    Record::new(
        file_name,
        FieldKey::Iloc,
        IconLocation::new(position.0, position.1),
    )
}

fn window_settings(position: (u32, u32), size: (u32, u32)) -> BrowserWindowSettings {
    BrowserWindowSettings {
        window_bounds: Some(WindowBounds {
            x: position.0.into(),
            y: position.1.into(),
            width: size.0.into(),
            height: size.1.into(),
        }),
        container_show_sidebar: Some(true),
        show_sidebar: Some(true),
        show_path_bar: Some(false),
        show_status_bar: Some(false),
        show_tab_view: Some(false),
        show_toolbar: Some(false),
        sidebar_width: Some(0.0),
        ..Default::default()
    }
}

fn icon_view(background_alias: Vec<u8>) -> IconViewProperties {
    IconViewProperties {
        background_type: Some(2),
        background_color: Some((1.0, 1.0, 1.0)),
        show_icon_preview: Some(true),
        show_item_info: Some(false),
        text_size: Some(12.0),
        icon_size: Some(ICON_SIZE),
        view_options_version: Some(1),
        grid_spacing: Some(100.0),
        grid_offset: Some((0.0, 0.0)),
        label_on_bottom: Some(true),
        arrange_by: Some("none".into()),
        unknown: [(
            "backgroundImageAlias".into(),
            PropertyList::Data(background_alias),
        )]
        .into(),
        ..Default::default()
    }
}

/// Finder finds the background through an alias that names it by catalog ID,
/// so the alias has to describe the image's own volume, not the host's.
fn background_alias(volume: &HfsPlus) -> crate::Result<AliasRecord> {
    let parent = volume.entry_info(&format!("/{BACKGROUND_FOLDER}"))?;
    let target_path = format!("/{BACKGROUND_FOLDER}/{BACKGROUND_FILE}");
    let target = volume.entry_info(&target_path)?;
    let volume_name = volume.volume_name();

    let alias_volume = AliasVolume::new(
        volume_name.as_bytes(),
        HfsTimestampSeconds::from(volume.volume_create_date()?),
        FilesystemSignature::HIERARCHICAL_FILE_SYSTEM_PLUS,
        DiskType::Ejectable,
    )
    .map_err(alias_error)?;
    let alias_target = AliasTarget::new(
        AliasKind::File,
        CatalogNodeId(parent.cnid),
        BACKGROUND_FILE.as_bytes(),
        CatalogNodeId(target.cnid),
        HfsTimestampSeconds::from(target.create_date),
    )
    .map_err(alias_error)?;
    let mount_path = format!("/Volumes/{volume_name}");
    let metadata = AliasMetadata::builder()
        .header(AliasHeader::new(alias_volume, alias_target))
        .parent_folder_name(Some(BACKGROUND_FOLDER.as_bytes().to_vec()))
        .catalog_node_id_path(vec![parent.cnid])
        .unicode_target_name(Some(BACKGROUND_FILE.into()))
        .unicode_volume_name(Some(volume_name.into()))
        .target_path(Some(target_path.into_bytes()))
        .volume_path(Some(
            VolumeMountPath::new(mount_path.as_bytes()).map_err(alias_error)?,
        ))
        .build();
    AliasRecord::from_metadata(metadata).map_err(alias_error)
}

fn alias_error(error: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("Failed to build the Finder alias: {error}")
}
