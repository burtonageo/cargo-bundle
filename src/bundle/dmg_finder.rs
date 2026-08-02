use crate::Settings;
use anyhow::Context;
use dmg_layout::AliasRecord;
use ds_parser::{FieldKey, Record, Value, write as write_dsstore};
use plist::Dictionary;
use resvg::tiny_skia::Pixmap;
use resvg::usvg::{Options, Transform, Tree};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use tiff::encoder::{Rational, TiffEncoder, colortype};
use tiff::tags::ResolutionUnit;

const ICON_SIZE: u64 = 48;

const WINDOW_POSITION: (u32, u32) = (100, 100);

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
    let background_alias = AliasRecord::for_target(&background_file, mount_point)
        .with_context(|| "Failed to build alias record for DMG background")?
        .to_bytes()
        .with_context(|| "Failed to serialise DMG background alias record")?;

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
