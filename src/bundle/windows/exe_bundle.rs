// Resource embedding is only performed when cargo-bundle runs on Windows.
// On other hosts the executable is still copied, but no icon or version info
// is injected into the PE binary.

use crate::Settings;
use crate::bundle::common;
use anyhow::Context;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use winres_edit::Resources;

#[cfg(target_os = "windows")]
use super::group_icon::GroupIcon;
#[cfg(target_os = "windows")]
use super::icon::Icon;
#[cfg(target_os = "windows")]
use resvg::tiny_skia::{Pixmap, Transform};
#[cfg(target_os = "windows")]
use resvg::usvg::{Options, Tree};

#[cfg(target_os = "windows")]
const ICON_PIXEL_SIZES: &[u32] = &[16, 24, 32, 48, 64, 96, 128, 256, 512];

#[cfg(target_os = "windows")]
const LANGUAGE_ID_ENGLISH_US: u16 = 0x0409;

#[cfg(target_os = "windows")]
const VERSION_NODE_DATA_TYPE_BINARY: u16 = 0;
#[cfg(target_os = "windows")]
const VERSION_NODE_DATA_TYPE_TEXT: u16 = 1;

/// Windows PE resource type identifier for RT_GROUP_ICON.
#[cfg(target_os = "windows")]
const RT_GROUP_ICON_RESOURCE_TYPE: u16 = 14;

#[cfg(target_os = "windows")]
const APPLICATION_ICON_GROUP_RESOURCE_ID: u16 = 1;
#[cfg(target_os = "windows")]
const FIRST_INDIVIDUAL_ICON_RESOURCE_ID: u16 = 1;
#[cfg(target_os = "windows")]
const VERSION_RESOURCE_ID: u16 = 1;

/// String table key encoding language 0x0409 (en-US) and code page 0x04B0 (Unicode UTF-16).
#[cfg(target_os = "windows")]
const STRING_TABLE_LOCALE_ENGLISH_US_UNICODE: &str = "040904B0";

/// Translation record for English (US), code page 1200 (Unicode UTF-16 LE).
/// Layout: [language_id_low, language_id_high, codepage_low, codepage_high]
#[cfg(target_os = "windows")]
const TRANSLATION_ENTRY_ENGLISH_US_UNICODE: [u8; 4] = [0x09, 0x04, 0xB0, 0x04];

#[cfg(target_os = "windows")]
const WINDOWS_VERSION_COMPONENT_COUNT: usize = 4;

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    let exe_name = format!("{}.exe", settings.binary_name());
    common::print_bundling(&exe_name)?;

    let base_dir = settings.project_out_directory().join("bundle/exe");
    fs::create_dir_all(&base_dir)
        .with_context(|| format!("Failed to create output directory {base_dir:?}"))?;

    let output_exe = base_dir.join(&exe_name);
    fs::copy(settings.binary_path(), &output_exe)
        .with_context(|| format!("Failed to copy executable to {output_exe:?}"))?;

    let svg_icon_path = settings
        .icon_files()
        .filter_map(|icon_path_result| icon_path_result.ok())
        .find(|path| path.extension() == Some(OsStr::new("svg")));

    embed_resources(settings, &output_exe, svg_icon_path.as_deref())?;

    Ok(vec![output_exe])
}

fn embed_resources(
    settings: &Settings,
    exe_path: &std::path::Path,
    svg_icon: Option<&std::path::Path>,
) -> crate::Result<()> {
    #[cfg(target_os = "windows")]
    {
        embed_resources_windows(settings, exe_path, svg_icon)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (settings, exe_path, svg_icon);
        common::print_warning(
            "Windows PE resource embedding (icons, version info) is only performed \
             when cargo-bundle itself is run on Windows.",
        )?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn embed_resources_windows(
    settings: &Settings,
    exe_path: &std::path::Path,
    svg_icon: Option<&std::path::Path>,
) -> crate::Result<()> {
    let mut resources = Resources::new(exe_path);
    resources
        .load()
        .with_context(|| "Failed to load PE resources")?;
    resources
        .open()
        .with_context(|| "Failed to open PE resources for writing")?;

    embed_version_info(settings, &resources)?;

    if let Some(svg_path) = svg_icon {
        embed_svg_icons(svg_path, &resources)?;
    }

    resources.close();

    Ok(())
}

#[cfg(target_os = "windows")]
fn embed_version_info(
    settings: &Settings,
    resources: &winres_edit::Resources,
) -> crate::Result<()> {
    use winres_edit::{Id, Resource, resource_type};

    let version_string = settings.version_string().to_string();
    let string_pairs: &[(&str, String)] = &[
        ("ProductName", settings.bundle_name().to_owned()),
        ("FileDescription", settings.short_description().to_owned()),
        ("FileVersion", version_string.clone()),
        ("ProductVersion", version_string),
        (
            "LegalCopyright",
            settings.copyright_string().unwrap_or("").to_owned(),
        ),
        (
            "CompanyName",
            settings.authors_comma_separated().unwrap_or_default(),
        ),
        ("InternalName", settings.binary_name().to_owned()),
        (
            "OriginalFilename",
            format!("{}.exe", settings.binary_name()),
        ),
    ];

    let version_info_data = build_version_info_resource(settings, string_pairs)
        .with_context(|| "Failed to build VS_VERSIONINFO resource")?;

    let version_resource = Resource::new(
        resources,
        resource_type::VERSION.into(),
        Id::Integer(VERSION_RESOURCE_ID).into(),
        LANGUAGE_ID_ENGLISH_US,
        &version_info_data,
    );

    version_resource
        .update()
        .with_context(|| "Failed to update version info resource")?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn embed_svg_icons(
    svg_path: &std::path::Path,
    resources: &winres_edit::Resources,
) -> crate::Result<()> {
    use winres_edit::{Id, Resource, resource_type};

    let svg_text = std::fs::read_to_string(svg_path)
        .with_context(|| format!("Failed to read SVG icon {svg_path:?}"))?;
    let svg_parse_options = Options::default();
    let svg_tree = Tree::from_data(svg_text.as_bytes(), &svg_parse_options)
        .with_context(|| "Failed to parse SVG icon data")?;

    let mut group_icon = GroupIcon::default();

    for (size_index, &pixel_size) in ICON_PIXEL_SIZES.iter().enumerate() {
        let icon_resource_id = FIRST_INDIVIDUAL_ICON_RESOURCE_ID + size_index as u16;

        let mut pixmap = Pixmap::new(pixel_size, pixel_size)
            .with_context(|| format!("Failed to create {pixel_size}×{pixel_size} pixmap"))?;

        let scale_x = pixel_size as f32 / svg_tree.size().width();
        let scale_y = pixel_size as f32 / svg_tree.size().height();
        resvg::render(
            &svg_tree,
            Transform::from_scale(scale_x, scale_y),
            &mut pixmap.as_mut(),
        );

        let icon = Icon::new_from_rgba(pixel_size, pixel_size, pixmap.data().to_vec());
        let encoded_icon = icon
            .encode()
            .with_context(|| format!("Failed to encode {pixel_size}×{pixel_size} icon"))?;

        group_icon.push_icon(
            icon.group_icon_entry(icon_resource_id)
                .with_context(|| "Failed to build group icon entry")?,
        );

        let icon_resource = Resource::new(
            resources,
            resource_type::ICON.into(),
            Id::Integer(icon_resource_id).into(),
            LANGUAGE_ID_ENGLISH_US,
            &encoded_icon,
        );

        icon_resource
            .update()
            .with_context(|| format!("Failed to embed {pixel_size}×{pixel_size} icon resource"))?;
    }

    let group_icon_data = group_icon
        .encode()
        .with_context(|| "Failed to encode RT_GROUP_ICON")?;

    let group_icon_resource = Resource::new(
        resources,
        Id::Integer(RT_GROUP_ICON_RESOURCE_TYPE).into(),
        Id::Integer(APPLICATION_ICON_GROUP_RESOURCE_ID).into(),
        LANGUAGE_ID_ENGLISH_US,
        &group_icon_data,
    );

    group_icon_resource
        .update()
        .with_context(|| "Failed to embed RT_GROUP_ICON resource")?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn pad_to_four_byte_alignment(buffer: &mut Vec<u8>) {
    const FOUR_BYTE_ALIGNMENT: usize = 4;
    while !buffer.len().is_multiple_of(FOUR_BYTE_ALIGNMENT) {
        buffer.push(0);
    }
}

#[cfg(target_os = "windows")]
fn encode_null_terminated_utf16_le(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0u16))
        .flat_map(|utf16_code_unit| utf16_code_unit.to_le_bytes())
        .collect()
}

#[cfg(target_os = "windows")]
fn pack_windows_version(major: u64, minor: u64, patch: u64, build: u64) -> u64 {
    (major << 48) | (minor << 32) | (patch << 16) | build
}

#[cfg(target_os = "windows")]
fn build_fixed_file_info(file_version: u64, product_version: u64) -> Vec<u8> {
    /// Magic signature that identifies a VS_FIXEDFILEINFO structure.
    const VS_FIXED_FILE_INFO_SIGNATURE: u32 = 0xFEEF_04BD;

    /// VS_FIXEDFILEINFO structure layout version 1.0.
    const VS_FIXED_FILE_INFO_STRUCT_VERSION_1_0: u32 = 0x0001_0000;

    const VS_FILE_FLAGS_MASK_ALL: u32 = 0xFFFF_FFFF;
    const VS_FILE_FLAGS_NONE: u32 = 0x0000_0000;

    /// Operating system identifier: Windows NT Win32 subsystem.
    const VOS_NT_WINDOWS32: u32 = 0x0000_0004;

    /// File type identifier: application executable.
    const VFT_APPLICATION: u32 = 0x0000_0001;

    const VFT2_UNKNOWN_SUBTYPE: u32 = 0x0000_0000;
    const VS_FILE_DATE_UNUSED: u32 = 0x0000_0000;

    let mut buffer = Vec::new();
    for field_value in [
        VS_FIXED_FILE_INFO_SIGNATURE,
        VS_FIXED_FILE_INFO_STRUCT_VERSION_1_0,
        (file_version >> 32) as u32,
        file_version as u32,
        (product_version >> 32) as u32,
        product_version as u32,
        VS_FILE_FLAGS_MASK_ALL,
        VS_FILE_FLAGS_NONE,
        VOS_NT_WINDOWS32,
        VFT_APPLICATION,
        VFT2_UNKNOWN_SUBTYPE,
        VS_FILE_DATE_UNUSED,
        VS_FILE_DATE_UNUSED,
    ] {
        buffer.extend_from_slice(&field_value.to_le_bytes());
    }
    buffer
}

#[cfg(target_os = "windows")]
fn parse_version(version_string: &str) -> u64 {
    let mut version_components = version_string
        .splitn(WINDOWS_VERSION_COMPONENT_COUNT, '.')
        .map(|component| component.parse::<u16>().unwrap_or(0));

    let major = version_components.next().unwrap_or(0) as u64;
    let minor = version_components.next().unwrap_or(0) as u64;
    let patch = version_components.next().unwrap_or(0) as u64;
    let build = version_components.next().unwrap_or(0) as u64;

    pack_windows_version(major, minor, patch, build)
}

/// Builds a VS_VERSIONINFO node per the Windows SDK specification:
/// <https://learn.microsoft.com/en-us/windows/win32/menurc/vs-versioninfo>
#[cfg(target_os = "windows")]
fn build_version_info_node(
    key: &str,
    value_bytes: &[u8],
    data_type: u16,
    children: &[u8],
) -> Vec<u8> {
    let key_encoded = encode_null_terminated_utf16_le(key);
    let header_byte_size = 2 + 2 + 2 + key_encoded.len();
    let total_byte_size = header_byte_size + value_bytes.len() + children.len();

    let mut buffer = Vec::new();
    buffer.extend_from_slice(&(total_byte_size as u16).to_le_bytes());
    buffer.extend_from_slice(&(value_bytes.len() as u16).to_le_bytes());
    buffer.extend_from_slice(&data_type.to_le_bytes());
    buffer.extend_from_slice(&key_encoded);

    pad_to_four_byte_alignment(&mut buffer);
    buffer.extend_from_slice(value_bytes);

    pad_to_four_byte_alignment(&mut buffer);
    buffer.extend_from_slice(children);
    buffer
}

#[cfg(target_os = "windows")]
fn build_string_entry(key: &str, value: &str) -> Vec<u8> {
    let key_encoded = encode_null_terminated_utf16_le(key);
    let value_encoded = encode_null_terminated_utf16_le(value);
    let value_char_count = (value.encode_utf16().count() + 1) as u16;
    let node_byte_length = (2 + 2 + 2 + key_encoded.len() + value_encoded.len()) as u16;

    let mut buffer = Vec::new();
    buffer.extend_from_slice(&node_byte_length.to_le_bytes());
    buffer.extend_from_slice(&value_char_count.to_le_bytes());
    buffer.extend_from_slice(&VERSION_NODE_DATA_TYPE_TEXT.to_le_bytes());
    buffer.extend_from_slice(&key_encoded);

    pad_to_four_byte_alignment(&mut buffer);
    buffer.extend_from_slice(&value_encoded);
    buffer
}

#[cfg(target_os = "windows")]
fn build_string_file_info(pairs: &[(&str, String)]) -> Vec<u8> {
    let mut string_entries = Vec::new();
    for (key, value) in pairs {
        pad_to_four_byte_alignment(&mut string_entries);
        string_entries.extend(build_string_entry(key, value));
    }

    let string_table = build_version_info_node(
        STRING_TABLE_LOCALE_ENGLISH_US_UNICODE,
        &[],
        VERSION_NODE_DATA_TYPE_TEXT,
        &string_entries,
    );
    build_version_info_node(
        "StringFileInfo",
        &[],
        VERSION_NODE_DATA_TYPE_TEXT,
        &string_table,
    )
}

#[cfg(target_os = "windows")]
fn build_var_file_info() -> Vec<u8> {
    let translation_node = build_version_info_node(
        "Translation",
        &TRANSLATION_ENTRY_ENGLISH_US_UNICODE,
        VERSION_NODE_DATA_TYPE_BINARY,
        &[],
    );
    build_version_info_node(
        "VarFileInfo",
        &[],
        VERSION_NODE_DATA_TYPE_TEXT,
        &translation_node,
    )
}

#[cfg(target_os = "windows")]
fn build_version_info_resource(
    settings: &Settings,
    string_pairs: &[(&str, String)],
) -> crate::Result<Vec<u8>> {
    let version = parse_version(&settings.version_string().to_string());
    let fixed_file_info = build_fixed_file_info(version, version);
    let string_file_info = build_string_file_info(string_pairs);
    let var_file_info = build_var_file_info();

    let children = [string_file_info.as_slice(), var_file_info.as_slice()].concat();
    let root_node = build_version_info_node(
        "VS_VERSION_INFO",
        &fixed_file_info,
        VERSION_NODE_DATA_TYPE_BINARY,
        &children,
    );

    Ok(root_node)
}
