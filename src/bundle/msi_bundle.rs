use super::common;
use super::settings::Settings;
use anyhow::Context;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

type Package = msi::Package<fs::File>;

// Don't add more files to a cabinet folder that already has this many bytes:
const CABINET_FOLDER_SIZE_LIMIT: u64 = 0x8000;
// The maximum number of resource files we'll put in one cabinet:
const CABINET_MAX_FILES: usize = 1000;
// The maximum number of data bytes we'll put in one cabinet:
const CABINET_MAX_SIZE: u64 = 0x1000_0000;

// File table attribute indicating that a file is "vital":
const FILE_ATTR_VITAL: u16 = 0x200;

// The name of the installer package's sole Feature:
const MAIN_FEATURE_NAME: &str = "MainFeature";

// A v4 UUID that was generated specifically for cargo-bundle, to be used as a
// namespace for generating v5 UUIDs from bundle identifier strings.
const UUID_NAMESPACE: [u8; 16] = [
    0xfd, 0x85, 0x95, 0xa8, 0x17, 0xa3, 0x47, 0x4e, 0xa6, 0x16, 0x76, 0x14, 0x8d, 0xfa, 0x0c, 0x7b,
];

// Info about a resource file (including the main executable) in the bundle.
struct ResourceInfo {
    // The path to the existing file that will be bundled as a resource.
    source_path: PathBuf,
    // Relative path from the install dir where this will be installed.
    dest_path: PathBuf,
    // The name of this resource file in the filesystem.
    filename: String,
    // The size of this resource file, in bytes.
    size: u64,
    // The database key for the Component that this resource is part of.
    component_key: String,
}

// Info about a directory that needs to be created during installation.
struct DirectoryInfo {
    // The database key for this directory.
    key: String,
    // The database key for this directory's parent.
    parent_key: String,
    // The name of this directory in the filesystem.
    name: String,
    // List of files in this directory, not counting subdirectories.
    files: Vec<String>,
}

// Info about a CAB archive within the installer package.
struct CabinetInfo {
    // The stream name for this cabinet.
    name: String,
    // The resource files that are in this cabinet.
    resources: Vec<ResourceInfo>,
}

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    common::print_warning("MSI bundle support is still experimental.")?;

    let msi_name = format!("{}.msi", settings.bundle_name());
    common::print_bundling(&msi_name)?;
    let base_dir = settings.project_out_directory().join("bundle/msi");
    let msi_path = base_dir.join(&msi_name);
    let mut package =
        new_empty_package(&msi_path).with_context(|| "Failed to initialize MSI package")?;

    // Generate package metadata:
    let guid = generate_package_guid(settings);
    set_summary_info(&mut package, guid, settings);
    create_property_table(&mut package, guid, settings)
        .with_context(|| "Failed to generate Property table")?;

    // Copy resource files into package:
    let mut resources = collect_resource_info(settings)
        .with_context(|| "Failed to collect resource file information")?;
    let directories = collect_directory_info(settings, &mut resources)
        .with_context(|| "Failed to collect resource directory information")?;
    let cabinets = divide_resources_into_cabinets(resources);
    generate_resource_cabinets(&mut package, &cabinets)
        .with_context(|| "Failed to generate resource cabinets")?;

    // Set up installer database tables:
    create_directory_table(&mut package, &directories)
        .with_context(|| "Failed to generate Directory table")?;
    create_feature_table(&mut package, settings)
        .with_context(|| "Failed to generate Feature table")?;
    create_component_table(&mut package, guid, &directories)
        .with_context(|| "Failed to generate Component table")?;
    create_feature_components_table(&mut package, &directories)
        .with_context(|| "Failed to generate FeatureComponents table")?;
    create_media_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate Media table")?;
    create_file_table(&mut package, &cabinets).with_context(|| "Failed to generate File table")?;
    create_install_execute_sequence_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate InstallExecuteSequence table")?;
    create_install_ui_sequence_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate InstallUISequence table")?;
    create_dialog_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate Dialog table")?;
    create_control_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate Control table")?;
    create_control_event_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate ControlEvent table")?;
    create_event_mapping_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate EventMapping table")?;
    create_text_style_table(&mut package, &cabinets)
        .with_context(|| "Failed to generate TextStyle table")?;
    // TODO: Create other needed tables.

    // Create app icon:
    package.create_table(
        "Icon",
        vec![
            msi::Column::build("Name").primary_key().id_string(72),
            msi::Column::build("Data").binary(),
        ],
    )?;
    let icon_name = format!("{}.ico", settings.binary_name());
    {
        let stream_name = format!("Icon.{icon_name}");
        let mut stream = package.write_stream(&stream_name)?;
        create_app_icon(&mut stream, settings)?;
    }
    package.insert_rows(
        msi::Insert::into("Icon").row(vec![msi::Value::Str(icon_name), msi::Value::from("Name")]),
    )?;

    package.flush()?;
    Ok(vec![msi_path])
}

fn new_empty_package(msi_path: &Path) -> crate::Result<Package> {
    if let Some(parent) = msi_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {parent:?}"))?;
    }
    let msi_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(msi_path)
        .with_context(|| format!("Failed to create file {msi_path:?}"))?;
    let package = msi::Package::create(msi::PackageType::Installer, msi_file)?;
    Ok(package)
}

// Generates a GUID for the package, based on `settings.bundle_identifier()`.
fn generate_package_guid(settings: &Settings) -> Uuid {
    let namespace = Uuid::from_bytes(UUID_NAMESPACE);
    Uuid::new_v5(&namespace, settings.bundle_identifier().as_bytes())
}

// Populates the summary metadata for the package from the bundle settings.
fn set_summary_info(package: &mut Package, package_guid: Uuid, settings: &Settings) {
    let summary_info = package.summary_info_mut();
    summary_info.set_creation_time_to_now();
    summary_info.set_subject(settings.bundle_name().to_string());
    summary_info.set_uuid(package_guid);
    summary_info.set_comments(settings.short_description().to_string());
    if let Some(authors) = settings.authors_comma_separated() {
        summary_info.set_author(authors);
    }
    let creating_app = crate::version_info!();
    summary_info.set_creating_application(creating_app);
    summary_info.set_word_count(2);
}

// Creates and populates the `Property` database table for the package.
fn create_property_table(
    package: &mut Package,
    package_guid: Uuid,
    settings: &Settings,
) -> crate::Result<()> {
    let authors = settings.authors_comma_separated().unwrap_or_default();
    package.create_table(
        "Property",
        vec![
            msi::Column::build("Property").primary_key().id_string(72),
            msi::Column::build("Value").text_string(0),
        ],
    )?;
    package.insert_rows(
        msi::Insert::into("Property")
            .row(vec![
                msi::Value::from("Manufacturer"),
                msi::Value::Str(authors),
            ])
            .row(vec![
                msi::Value::from("ProductCode"),
                msi::Value::from(package_guid),
            ])
            .row(vec![
                msi::Value::from("ProductLanguage"),
                msi::Value::from(msi::Language::from_tag("en-US")),
            ])
            .row(vec![
                msi::Value::from("ProductName"),
                msi::Value::from(settings.bundle_name()),
            ])
            .row(vec![
                msi::Value::from("ProductVersion"),
                msi::Value::from(settings.version_string().to_string()),
            ])
            .row(vec![
                msi::Value::from("DefaultUIFont"),
                msi::Value::from("DefaultFont"),
            ])
            .row(vec![msi::Value::from("Mode"), msi::Value::from("Install")])
            .row(vec![
                msi::Value::from("Text_action"),
                msi::Value::from("installation"),
            ])
            .row(vec![
                msi::Value::from("Text_agent"),
                msi::Value::from("installer"),
            ])
            .row(vec![
                msi::Value::from("Text_Doing"),
                msi::Value::from("installing"),
            ])
            .row(vec![
                msi::Value::from("Text_done"),
                msi::Value::from("installed"),
            ]),
    )?;
    Ok(())
}

// Returns a list of `ResourceInfo` structs for the binary executable and all
// the resource files that should be included in the package.
fn collect_resource_info(settings: &Settings) -> crate::Result<Vec<ResourceInfo>> {
    let mut resources = Vec::<ResourceInfo>::new();
    resources.push(ResourceInfo {
        source_path: settings.binary_path().to_path_buf(),
        dest_path: PathBuf::from(settings.binary_name()),
        filename: settings.binary_name().to_string(),
        size: settings.binary_path().metadata()?.len(),
        component_key: String::new(),
    });
    let root_rsrc_dir = PathBuf::from("Resources");
    for source_path in settings.resource_files() {
        let source_path = source_path?;
        let metadata = source_path.metadata()?;
        let size = metadata.len();
        let dest_path = root_rsrc_dir.join(common::resource_relpath(&source_path));
        let filename = dest_path.file_name().unwrap().to_string_lossy().to_string();
        let info = ResourceInfo {
            source_path,
            dest_path,
            filename,
            size,
            component_key: String::new(),
        };
        resources.push(info);
    }
    Ok(resources)
}

// Based on the list of all resource files to be bundled, returns a list of
// all the directories that need to be created during installation.  Also,
// modifies each `ResourceInfo` object to populate its `component_key` field
// with the database key of the Component that the resource will be associated
// with.
fn collect_directory_info(
    settings: &Settings,
    resources: &mut [ResourceInfo],
) -> crate::Result<Vec<DirectoryInfo>> {
    let mut dir_map = BTreeMap::<PathBuf, DirectoryInfo>::new();
    let mut dir_index: i32 = 0;
    dir_map.insert(
        PathBuf::new(),
        DirectoryInfo {
            key: "INSTALLDIR".to_string(),
            parent_key: "ProgramFilesFolder".to_string(),
            name: settings.bundle_name().to_string(),
            files: Vec::new(),
        },
    );
    for resource in resources.iter_mut() {
        let mut dir_key = "INSTALLDIR".to_string();
        let mut dir_path = PathBuf::new();
        for component in resource.dest_path.parent().unwrap().components() {
            if let std::path::Component::Normal(name) = component {
                dir_path.push(name);
                if dir_map.contains_key(&dir_path) {
                    dir_key.clone_from(&dir_map.get(&dir_path).unwrap().key);
                } else {
                    let new_key = format!("RDIR{dir_index:04}");
                    dir_map.insert(
                        dir_path.clone(),
                        DirectoryInfo {
                            key: new_key.clone(),
                            parent_key: dir_key.clone(),
                            name: name.to_string_lossy().to_string(),
                            files: Vec::new(),
                        },
                    );
                    dir_key = new_key;
                    dir_index += 1;
                }
            }
        }
        let directory = dir_map.get_mut(&dir_path).unwrap();
        debug_assert_eq!(directory.key, dir_key);
        directory.files.push(resource.filename.clone());
        resource.component_key = dir_key.to_string();
    }
    Ok(dir_map.into_values().collect())
}

// Divides up the list of resource into some number of cabinets, subject to a
// few constraints: 1) no one cabinet will have two resources with the same
// filename, 2) no one cabinet will have more than `CABINET_MAX_FILES` files
// in it, and 3) no one cabinet will contain more than `CABINET_MAX_SIZE`
// bytes of data (unless that cabinet consists of a single file that is
// already bigger than that).
fn divide_resources_into_cabinets(mut resources: Vec<ResourceInfo>) -> Vec<CabinetInfo> {
    let mut cabinets = Vec::new();
    while !resources.is_empty() {
        let mut filenames = HashSet::<String>::new();
        let mut total_size = 0;
        let mut leftovers = Vec::<ResourceInfo>::new();
        let mut cabinet = CabinetInfo {
            name: format!("rsrc{:04}.cab", cabinets.len()),
            resources: Vec::new(),
        };
        for resource in resources.into_iter() {
            if cabinet.resources.len() >= CABINET_MAX_FILES
                || (!cabinet.resources.is_empty() && total_size + resource.size > CABINET_MAX_SIZE)
                || filenames.contains(&resource.filename)
            {
                leftovers.push(resource);
            } else {
                filenames.insert(resource.filename.clone());
                total_size += resource.size;
                cabinet.resources.push(resource);
            }
        }
        cabinets.push(cabinet);
        resources = leftovers;
    }
    cabinets
}

// Creates the CAB archives within the package that contain the binary
// executable and all the resource files.
fn generate_resource_cabinets(
    package: &mut Package,
    cabinets: &[CabinetInfo],
) -> crate::Result<()> {
    for cabinet_info in cabinets.iter() {
        let mut builder = cab::CabinetBuilder::new();
        let mut file_map = HashMap::<String, &Path>::new();
        let mut resource_index: usize = 0;
        while resource_index < cabinet_info.resources.len() {
            let folder = builder.add_folder(cab::CompressionType::MsZip);
            let mut folder_size: u64 = 0;
            while resource_index < cabinet_info.resources.len()
                && folder_size < CABINET_FOLDER_SIZE_LIMIT
            {
                let resource = &cabinet_info.resources[resource_index];
                folder_size += resource.size;
                folder.add_file(resource.filename.as_str());
                debug_assert!(!file_map.contains_key(&resource.filename));
                file_map.insert(resource.filename.clone(), &resource.source_path);
                resource_index += 1;
            }
        }
        let stream = package.write_stream(cabinet_info.name.as_str())?;
        let mut cabinet_writer = builder.build(stream)?;
        while let Some(mut file_writer) = cabinet_writer.next_file()? {
            debug_assert!(file_map.contains_key(file_writer.file_name()));
            let file_path = file_map.get(file_writer.file_name()).unwrap();
            let mut file = fs::File::open(file_path)?;
            io::copy(&mut file, &mut file_writer)?;
        }
        cabinet_writer.finish()?;
    }
    Ok(())
}

// Creates and populates the `Directory` database table for the package.
fn create_directory_table(
    package: &mut Package,
    directories: &[DirectoryInfo],
) -> crate::Result<()> {
    package.create_table(
        "Directory",
        vec![
            msi::Column::build("Directory").primary_key().id_string(72),
            msi::Column::build("Directory_Parent")
                .nullable()
                .foreign_key("Directory", 1)
                .id_string(72),
            msi::Column::build("DefaultDir")
                .category(msi::Category::DefaultDir)
                .string(255),
        ],
    )?;
    let mut rows = Vec::new();
    for directory in directories.iter() {
        rows.push(vec![
            msi::Value::Str(directory.key.clone()),
            msi::Value::Str(directory.parent_key.clone()),
            msi::Value::Str(directory.name.clone()),
        ]);
    }
    package.insert_rows(
        msi::Insert::into("Directory")
            .row(vec![
                msi::Value::from("TARGETDIR"),
                msi::Value::Null,
                msi::Value::from("SourceDir"),
            ])
            .row(vec![
                msi::Value::from("ProgramFilesFolder"),
                msi::Value::from("TARGETDIR"),
                msi::Value::from("."),
            ])
            .rows(rows),
    )?;
    Ok(())
}

// Creates and populates the `Feature` database table for the package.  The
// package will have a single main feature that installs everything.
fn create_feature_table(package: &mut Package, settings: &Settings) -> crate::Result<()> {
    package.create_table(
        "Feature",
        vec![
            msi::Column::build("Feature").primary_key().id_string(38),
            msi::Column::build("Feature_Parent")
                .nullable()
                .foreign_key("Feature", 1)
                .id_string(38),
            msi::Column::build("Title").nullable().text_string(64),
            msi::Column::build("Description")
                .nullable()
                .text_string(255),
            msi::Column::build("Display")
                .nullable()
                .range(0, 0x7fff)
                .int16(),
            msi::Column::build("Level").range(0, 0x7fff).int16(),
            msi::Column::build("Directory_")
                .nullable()
                .foreign_key("Directory", 1)
                .id_string(72),
            msi::Column::build("Attributes").int16(),
        ],
    )?;
    package.insert_rows(msi::Insert::into("Feature").row(vec![
        msi::Value::from(MAIN_FEATURE_NAME),
        msi::Value::Null,
        msi::Value::from(settings.bundle_name()),
        msi::Value::Null,
        msi::Value::Int(1),
        msi::Value::Int(1),
        msi::Value::from("INSTALLDIR"),
        msi::Value::Int(24),
    ]))?;
    Ok(())
}

// Creates and populates the `Component` database table for the package.  One
// component is created for each subdirectory under in the install dir.
fn create_component_table(
    package: &mut Package,
    package_guid: Uuid,
    directories: &[DirectoryInfo],
) -> crate::Result<()> {
    package.create_table(
        "Component",
        vec![
            msi::Column::build("Component").primary_key().id_string(72),
            msi::Column::build("ComponentId")
                .nullable()
                .category(msi::Category::Guid)
                .string(38),
            msi::Column::build("Directory_")
                .nullable()
                .foreign_key("Directory", 1)
                .id_string(72),
            msi::Column::build("Attributes").int16(),
            msi::Column::build("Condition")
                .nullable()
                .category(msi::Category::Condition)
                .string(255),
            msi::Column::build("KeyPath").nullable().id_string(72),
        ],
    )?;
    let mut rows = Vec::new();
    for directory in directories.iter() {
        if !directory.files.is_empty() {
            let hash_input = directory.files.join("/");
            let uuid = Uuid::new_v5(&package_guid, hash_input.as_bytes());
            rows.push(vec![
                msi::Value::Str(directory.key.clone()),
                msi::Value::from(uuid),
                msi::Value::Str(directory.key.clone()),
                msi::Value::Int(0),
                msi::Value::Null,
                msi::Value::Str(directory.files[0].clone()),
            ]);
        }
    }
    package.insert_rows(msi::Insert::into("Component").rows(rows))?;
    Ok(())
}

// Creates and populates the `FeatureComponents` database table for the
// package.  All components are added to the package's single main feature.
fn create_feature_components_table(
    package: &mut Package,
    directories: &[DirectoryInfo],
) -> crate::Result<()> {
    package.create_table(
        "FeatureComponents",
        vec![
            msi::Column::build("Feature_")
                .primary_key()
                .foreign_key("Component", 1)
                .id_string(38),
            msi::Column::build("Component_")
                .primary_key()
                .foreign_key("Component", 1)
                .id_string(72),
        ],
    )?;
    let mut rows = Vec::new();
    for directory in directories.iter() {
        if !directory.files.is_empty() {
            rows.push(vec![
                msi::Value::from(MAIN_FEATURE_NAME),
                msi::Value::Str(directory.key.clone()),
            ]);
        }
    }
    package.insert_rows(msi::Insert::into("FeatureComponents").rows(rows))?;
    Ok(())
}

// Creates and populates the `Media` database table for the package, with one
// entry for each CAB archive within the package.
fn create_media_table(package: &mut Package, cabinets: &[CabinetInfo]) -> crate::Result<()> {
    package.create_table(
        "Media",
        vec![
            msi::Column::build("DiskId")
                .primary_key()
                .range(1, 0x7fff)
                .int16(),
            msi::Column::build("LastSequence").range(0, 0x7fff).int16(),
            msi::Column::build("DiskPrompt").nullable().text_string(64),
            msi::Column::build("Cabinet")
                .nullable()
                .category(msi::Category::Cabinet)
                .string(255),
            msi::Column::build("VolumeLabel").nullable().text_string(32),
            msi::Column::build("Source")
                .nullable()
                .category(msi::Category::Property)
                .string(32),
        ],
    )?;
    let mut disk_id: i32 = 0;
    let mut last_seq: i32 = 0;
    let mut rows = Vec::new();
    for cabinet in cabinets.iter() {
        disk_id += 1;
        last_seq += cabinet.resources.len() as i32;
        rows.push(vec![
            msi::Value::Int(disk_id),
            msi::Value::Int(last_seq),
            msi::Value::Null,
            msi::Value::Str(format!("#{}", cabinet.name)),
            msi::Value::Null,
            msi::Value::Null,
        ]);
    }
    package.insert_rows(msi::Insert::into("Media").rows(rows))?;
    Ok(())
}

// Creates and populates the `File` database table for the package, with one
// entry for each resource file to be installed (including the main
// executable).
fn create_file_table(package: &mut Package, cabinets: &[CabinetInfo]) -> crate::Result<()> {
    package.create_table(
        "File",
        vec![
            msi::Column::build("File").primary_key().id_string(72),
            msi::Column::build("Component_")
                .foreign_key("Component", 1)
                .id_string(72),
            msi::Column::build("FileName")
                .category(msi::Category::Filename)
                .string(255),
            msi::Column::build("FileSize").range(0, 0x7fffffff).int32(),
            msi::Column::build("Version")
                .nullable()
                .category(msi::Category::Version)
                .string(72),
            msi::Column::build("Language")
                .nullable()
                .category(msi::Category::Language)
                .string(20),
            msi::Column::build("Attributes")
                .nullable()
                .range(0, 0x7fff)
                .int16(),
            msi::Column::build("Sequence").range(1, 0x7fff).int16(),
        ],
    )?;
    let mut rows = Vec::new();
    let mut sequence: i32 = 1;
    for cabinet in cabinets.iter() {
        for resource in cabinet.resources.iter() {
            rows.push(vec![
                msi::Value::Str(resource.filename.clone()),
                msi::Value::Str(resource.component_key.clone()),
                msi::Value::Str(resource.filename.clone()),
                msi::Value::Int(resource.size as i32),
                msi::Value::Null,
                msi::Value::Null,
                msi::Value::from(FILE_ATTR_VITAL),
                msi::Value::Int(sequence),
            ]);
            sequence += 1;
        }
    }
    package.insert_rows(msi::Insert::into("File").rows(rows))?;
    Ok(())
}

fn create_install_execute_sequence_table(
    package: &mut Package,
    _cabinets: &[CabinetInfo],
) -> crate::Result<()> {
    package.create_table(
        "InstallExecuteSequence",
        vec![
            msi::Column::build("Action").primary_key().id_string(72),
            msi::Column::build("Condition")
                .nullable()
                .category(msi::Category::Condition)
                .string(255),
            msi::Column::build("Sequence")
                .nullable()
                .range(-4, 0x7fff)
                .int16(),
        ],
    )?;
    let mut rows = Vec::new();
    let actions: [(&str, &str, i32); 24] = [
        //("LaunchConditions", "", 100), // Requires a LaunchCondition table
        //("FindRelatedProducts", "", 200), // Requires an Upgrade table
        //("AppSearch", "", 400), // Requires a Signature table
        //("CCPSearch", "NOT Installed", 500), // Requires a Signature or *Locator table
        //("RMCCPSearch", "NOT Installed", 600), // Requires the CCP_DRIVE property and a DrLocator table
        ("ValidateProductID", "", 700),
        ("CostInitialize", "", 800),
        ("FileCost", "", 900),
        ("CostFinalize", "", 1000),
        ("SetODBCFolders", "", 1100),
        //("MigrateFeatureStates", "", 1200),
        ("InstallValidate", "", 1400),
        ("InstallInitialize", "", 1500),
        ("AllocateRegistrySpace", "NOT Installed", 1550),
        ("ProcessComponents", "", 1600),
        ("UnpublishComponents", "", 1700),
        ("UnpublishFeatures", "", 1800),
        //("StopServices", "VersionNT", 1900), // Requires a ServiceControl table
        //("DeleteServices", "VersionNT", 2000), // Requires a ServiceControl table
        ("UnregisterComPlus", "", 2100),
        //("SelfUnregModules", "", 2200), // Requires a SelfReg table
        //("UnregisterTypeLibraries", "", 2300), // Requires a TypeLib table
        //("RemoveODBC", "", 2400), // Requires an ODBC* table
        //("UnregisterFonts", "", 2500), // Requires a Font table
        //("RemoveRegistryValues", "", 2600), // Requires a Registry table
        //("UnregisterClassInfo", "", 2700), // Requires a Class table
        //("UnregisterExtensionInfo", "", 2800), // Requires an Extension table
        //("UnregisterProgIdInfo", "", 2900), // Requires ProgId, Extension or Class table
        //("UnregisterMIMEInfo", "", 3000), // Requires a MIME table
        //("RemoveIniValues", "", 3100), // Requires an IniFile table
        //("RemoveShortcuts", "", 3200), // Requires a Shortcut table
        //("RemoveEnvironmentStrings", "", 3300), // Requires an Environment table
        //("RemoveDuplicateFiles", "", 3400), // Requires a DuplicateFile table
        ("RemoveFiles", "", 3500),
        ("RemoveFolders", "", 3600),
        ("CreateFolders", "", 3700),
        ("MoveFiles", "", 3800),
        ("InstallFiles", "", 4000),
        //("PatchFiles", "", 4090), // Requires a Patch table
        //("DuplicateFiles", "", 4210), // Requires a DuplicateFile table
        //("BindImage", "", 4300), // Requires a BindImage table
        //("CreateShortcuts", "", 4500), // Requires a Shortcut table
        //("RegisterClassInfo", "", 4600), // Requires a Class table
        //("RegisterExtensionInfo", "", 4700), // Requires an Extension table
        //("RegisterProgIdInfo", "", 4800), // Requires a ProgId table
        //("RegisterMIMEInfo", "", 4900), // Requires a MIME table
        //("WriteRegistryValues", "", 5000), // Requires a Registry table
        //("WriteIniValues", "", 5100), // Requires an IniFile table
        //("WriteEnvironmentStrings", "", 5200), // Requires an Environment table
        //("RegisterFonts", "", 5300), // Requires a Font table
        //("InstallODBC", "", 5400), // Requires an ODBC* table
        //("RegisterTypeLibraries", "", 5500), // Requires a TypeLib table
        //("SelfRegModules", "", 5600), // Requires a SelfReg table
        ("RegisterComPlus", "", 5700),
        //("InstallServices", "VersionNT", 5800), // Requires a ServiceInstall table
        //("StartServices", "VersionNT", 5900), // Requires a SelfReg ServiceControl
        ("RegisterUser", "", 6000),
        ("RegisterProduct", "", 6100),
        ("PublishComponents", "", 6200),
        ("PublishFeatures", "", 6300),
        ("PublishProduct", "", 6400),
        ("InstallFinalize", "", 6600),
        //("RemoveExistingProducts", "", 6700), // Requires an Upgrade table
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            if !action.1.is_empty() {
                msi::Value::Str(action.1.to_string())
            } else {
                msi::Value::Null
            },
            msi::Value::Int(action.2),
        ]);
    }
    package.insert_rows(msi::Insert::into("InstallExecuteSequence").rows(rows))?;
    Ok(())
}

fn create_install_ui_sequence_table(
    package: &mut Package,
    _cabinets: &[CabinetInfo],
) -> crate::Result<()> {
    package.create_table(
        "InstallUISequence",
        vec![
            msi::Column::build("Action").primary_key().id_string(72),
            msi::Column::build("Condition")
                .nullable()
                .category(msi::Category::Condition)
                .string(255),
            msi::Column::build("Sequence")
                .nullable()
                .range(-4, 0x7fff)
                .int16(),
        ],
    )?;
    let mut rows = Vec::new();
    let actions: [(&str, &str, i32); 9] = [
        ("FatalErrorDialog", "", -3),
        ("ExitDialog", "", -1),
        //("LaunchConditions", "", 100), // Requires a LaunchCondition table
        //("FindRelatedProducts", "", 200), // Requires an Upgrade table
        //("AppSearch", "", 400), // Requires a Signature table
        //("CCPSearch", "NOT Installed", 500), // Requires a Signature or *Locator table
        //("RMCCPSearch", "NOT Installed", 600), // Requires the CCP_DRIVE property and a DrLocator table
        ("CostInitialize", "", 800),
        ("FileCost", "", 900),
        ("CostFinalize", "", 1000),
        //("MigrateFeatureStates", "", 1200),
        ("WelcomeDialog", "NOT Installed", 1230),
        ("RemoveDialog", "Installed", 1240),
        ("ProgressDialog", "", 1280),
        ("ExecuteAction", "", 1300),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            if !action.1.is_empty() {
                msi::Value::Str(action.1.to_string())
            } else {
                msi::Value::Null
            },
            msi::Value::Int(action.2),
        ]);
    }
    package.insert_rows(msi::Insert::into("InstallUISequence").rows(rows))?;
    Ok(())
}

fn create_dialog_table(package: &mut Package, _cabinets: &[CabinetInfo]) -> crate::Result<()> {
    package.create_table(
        "Dialog",
        vec![
            msi::Column::build("Dialog").primary_key().id_string(72),
            msi::Column::build("HCentering").range(0, 100).int16(),
            msi::Column::build("VCentering").range(0, 100).int16(),
            msi::Column::build("Width").range(0, 0x7fff).int16(),
            msi::Column::build("Height").range(0, 0x7fff).int16(),
            msi::Column::build("Attributes")
                .nullable()
                .range(-4, 0x7fffffff)
                .int32(),
            msi::Column::build("Title")
                .nullable()
                .category(msi::Category::Formatted)
                .string(128),
            msi::Column::build("Control_First")
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Control_Default")
                .nullable()
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Control_Cancel")
                .nullable()
                .category(msi::Category::Identifier)
                .string(50),
        ],
    )?;
    let mut rows = Vec::new();
    type DialogTableEntry<'a> = (
        &'a str,
        i32,
        i32,
        i32,
        i32,
        i32,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
    );
    #[rustfmt::skip]
    let actions: [DialogTableEntry; 6] = [
        ("WelcomeDialog", 50, 50, 370, 270, 3, "[ProductName] Setup", "WelcomeInstall", "WelcomeInstall", "WelcomeInstall"),
        ("RemoveDialog", 50, 50, 370, 270, 3, "[ProductName] Setup", "RemoveRemove", "RemoveRemove", "RemoveRemove"),
        ("CancelDialog", 50, 10, 260, 85, 3, "[ProductName] Setup", "CancelNo", "CancelNo", "CancelNo"),
        ("ProgressDialog", 50, 50, 370, 270, 1, "[ProductName] Setup", "ProgressCancel", "ProgressCancel", "ProgressCancel"),
        ("ExitDialog", 50, 50, 370, 270, 3, "[ProductName] Setup", "ExitFinish", "ExitFinish", "ExitFinish"),
        ("FatalErrorDialog", 50, 50, 370, 270, 3, "[ProductName] Setup", "FatalFinish", "FatalFinish", "FatalFinish"),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            msi::Value::Int(action.1),
            msi::Value::Int(action.2),
            msi::Value::Int(action.3),
            msi::Value::Int(action.4),
            msi::Value::Int(action.5),
            msi::Value::Str(action.6.to_string()),
            msi::Value::Str(action.7.to_string()),
            if !action.8.is_empty() {
                msi::Value::Str(action.8.to_string())
            } else {
                msi::Value::Null
            },
            if !action.9.is_empty() {
                msi::Value::Str(action.9.to_string())
            } else {
                msi::Value::Null
            },
        ]);
    }
    package.insert_rows(msi::Insert::into("Dialog").rows(rows))?;
    Ok(())
}

fn create_control_table(package: &mut Package, _cabinets: &[CabinetInfo]) -> crate::Result<()> {
    package.create_table(
        "Control",
        vec![
            msi::Column::build("Dialog_").id_string(72),
            msi::Column::build("Control")
                .primary_key()
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Type")
                .category(msi::Category::Identifier)
                .string(20),
            msi::Column::build("X").range(0, 0x7fff).int16(),
            msi::Column::build("Y").range(0, 0x7fff).int16(),
            msi::Column::build("Width").range(0, 0x7fff).int16(),
            msi::Column::build("Height").range(0, 0x7fff).int16(),
            msi::Column::build("Attributes")
                .nullable()
                .range(-4, 0x7fffffff)
                .int32(),
            msi::Column::build("Property")
                .nullable()
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Text")
                .nullable()
                .category(msi::Category::Formatted)
                .string(0),
            msi::Column::build("Control_Next")
                .nullable()
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Help")
                .nullable()
                .category(msi::Category::Text)
                .string(50),
        ],
    )?;
    let mut rows = Vec::new();
    type ControlTableEntry<'a> = (
        &'a str,
        &'a str,
        &'a str,
        i32,
        i32,
        i32,
        i32,
        i32,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
    );
    let actions: [ControlTableEntry; 38] = [
        ("WelcomeDialog", "WelcomeDescription", "Text", 135, 70, 220, 50, 196611, "", "{\\DefaultFont}This will install [ProductName] on your computer. Click Install to continue or Cancel to exit the installer.", "", ""),
        ("WelcomeDialog", "WelcomeTitle", "Text", 135, 20, 220, 60, 196611, "", "{\\TitleFont}Welcome to the [ProductName] setup wizard", "", ""),
        ("WelcomeDialog", "WelcomeCancel", "PushButton", 304, 243, 56, 17, 3, "", "Cancel", "", ""),
        //("WelcomeDialog", "WelcomeBitmap", "Bitmap", 0, 0, 370, 234, 1, "", "[DialogBitmap]", "WelcomeBack", ""),
        ("WelcomeDialog", "WelcomeBack", "PushButton", 180, 243, 56, 17, 1, "", "Back", "WelcomeInstall", ""),
        ("WelcomeDialog", "WelcomeBottomLine", "Line", 0, 234, 374, 0, 1, "", "", "", ""),
        ("WelcomeDialog", "WelcomeInstall", "PushButton", 236, 243, 56, 17, 3, "", "Install", "WelcomeCancel", ""),
        ("RemoveDialog", "RemoveDescription", "Text", 135, 70, 220, 50, 196611, "", "This will remove [ProductName] from your computer. Click Remove to continue or Cancel to exit the uninstaller.", "", ""),
        ("RemoveDialog", "RemoveTitle", "Text", 135, 20, 220, 60, 196611, "", "{\\TitleFont}Uninstall [ProductName]", "", ""),
        ("RemoveDialog", "RemoveCancel", "PushButton", 304, 243, 56, 17, 3, "", "Cancel", "", ""),
        //("RemoveDialog", "RemoveBitmap", "Bitmap", 0, 0, 370, 234, 1, "", "[DialogBitmap]", "RemoveBack", ""),
        ("RemoveDialog", "RemoveBack", "PushButton", 180, 243, 56, 17, 1, "", "Back", "RemoveRemove", ""),
        ("RemoveDialog", "RemoveBottomLine", "Line", 0, 234, 374, 0, 1, "", "", "", ""),
        ("RemoveDialog", "RemoveRemove", "PushButton", 236, 243, 56, 17, 3, "", "Remove", "RemoveCancel", ""),
        //("CancelDialog", "CancelIcon", "Icon", 15, 15, 24, 24, 5242881, "", "[InfoIcon]", "", "Information icon|"),
        ("CancelDialog", "CancelNo", "PushButton", 132, 57, 56, 17, 3, "", "Continue", "CancelYes", ""),
        ("CancelDialog", "CancelText", "Text", 48, 15, 194, 30, 3, "", "Do you want to abort [ProductName] [Text_action]?", "", ""),
        ("CancelDialog", "CancelYes", "PushButton", 72, 57, 56, 17, 3, "", "Abort", "CancelNo", ""),
        ("ProgressDialog", "ProgressTitle", "Text", 20, 15, 200, 15, 196611, "", "{\\BoldFont}[Text_Doing] [ProductName]", "", ""),
        //("ProgressDialog", "ProgressBannerBitmap", "Bitmap", 0, 0, 374, 44, 1, "", "[BannerBitmap]", "ProgressBack", ""),
        ("ProgressDialog", "ProgressCancel", "PushButton", 304, 243, 56, 17, 3, "", "Cancel", "", ""),
        ("ProgressDialog", "ProgressText", "Text", 35, 65, 300, 25, 3, "", "Please wait while [ProductName] is [Text_done]. This may take several minutes.", "", ""),
        ("ProgressDialog", "ProgressActionText", "Text", 70, 105, 265, 15, 3, "", "", "", ""),
        ("ProgressDialog", "ProgressBack", "PushButton", 180, 243, 56, 17, 1, "", "Back", "ProgressNext", ""),
        ("ProgressDialog", "ProgressBottomLine", "Line", 0, 234, 374, 0, 1, "", "", "ProgressNext", ""),
        ("ProgressDialog", "ProgressNext", "PushButton", 236, 243, 56, 17, 1, "", "Next", "ProgressCancel", ""),
        ("ProgressDialog", "ProgressBannerLine", "Line", 0, 44, 374, 0, 1, "", "", "", ""),
        ("ProgressDialog", "ProgressProgressBar", "ProgressBar", 35, 125, 300, 10, 65537, "", "Progress done", "", ""),
        ("ProgressDialog", "ProgressStatusLabel", "Text", 35, 105, 35, 10, 3, "", "Status:", "", ""),
        ("ExitDialog", "ExitDescription", "Text", 135, 70, 220, 20, 196611, "", "Click the Finish button to exit the [Text_agent].", "", ""),
        ("ExitDialog", "ExitTitle", "Text", 135, 20, 220, 60, 196611, "", "{\\TitleFont}[ProductName] [Text_action] complete", "", ""),
        ("ExitDialog", "ExitCancel", "PushButton", 304, 243, 56, 17, 1, "", "Cancel", "", ""),
        //("ExitDialog", "ExitBitmap", "Bitmap", 0, 0, 370, 234, 1, "", "[DialogBitmap]", "ExitBack", ""),
        ("ExitDialog", "ExitBack", "PushButton", 180, 243, 56, 17, 1, "", "Back", "ExitFinish", ""),
        ("ExitDialog", "ExitBottomLine", "Line", 0, 234, 374, 0, 1, "", "", "", ""),
        ("ExitDialog", "ExitFinish", "PushButton", 236, 243, 56, 17, 3, "", "Finish", "ExitCancel", ""),
        ("FatalErrorDialog", "FatalTitle", "Text", 135, 20, 220, 60, 196611, "", "{\\TitleFont}[ProductName] [Text_agent] ended prematurely", "", ""),
        ("FatalErrorDialog", "FatalCancel", "PushButton", 304, 243, 56, 17, 1, "", "Cancel", "", ""),
        //("FatalErrorDialog", "FatalBitmap", "Bitmap", 0, 0, 370, 234, 1, "", "[DialogBitmap]", "FatalBack", ""),
        ("FatalErrorDialog", "FatalBack", "PushButton", 180, 243, 56, 17, 1, "", "Back", "FatalFinish", ""),
        ("FatalErrorDialog", "FatalBottomLine", "Line", 0, 234, 374, 0, 1, "", "", "", ""),
        ("FatalErrorDialog", "FatalFinish", "PushButton", 236, 243, 56, 17, 3, "", "Finish", "FatalCancel", ""),
        ("FatalErrorDialog", "FatalDescription1", "Text", 135, 70, 220, 40, 196611, "", "[ProductName] [Text_action] ended because of an error. The program has not been installed. This installer can be run again at a later time.", "", ""),
        ("FatalErrorDialog", "FatalDescription2", "Text", 135, 115, 220, 20, 196611, "", "Click the Finish button to exit the [Text_agent].", "", ""),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            msi::Value::Str(action.1.to_string()),
            msi::Value::Str(action.2.to_string()),
            msi::Value::Int(action.3),
            msi::Value::Int(action.4),
            msi::Value::Int(action.5),
            msi::Value::Int(action.6),
            msi::Value::Int(action.7),
            if !action.8.is_empty() {
                msi::Value::Str(action.8.to_string())
            } else {
                msi::Value::Null
            },
            if !action.9.is_empty() {
                msi::Value::Str(action.9.to_string())
            } else {
                msi::Value::Null
            },
            if !action.10.is_empty() {
                msi::Value::Str(action.10.to_string())
            } else {
                msi::Value::Null
            },
            if !action.11.is_empty() {
                msi::Value::Str(action.11.to_string())
            } else {
                msi::Value::Null
            },
        ]);
    }
    package.insert_rows(msi::Insert::into("Control").rows(rows))?;
    Ok(())
}

fn create_control_event_table(
    package: &mut Package,
    _cabinets: &[CabinetInfo],
) -> crate::Result<()> {
    package.create_table(
        "ControlEvent",
        vec![
            msi::Column::build("Dialog_").id_string(72),
            msi::Column::build("Control_")
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Event")
                .category(msi::Category::Formatted)
                .string(50),
            msi::Column::build("Argument")
                .category(msi::Category::Formatted)
                .string(255),
            msi::Column::build("Condition")
                .nullable()
                .category(msi::Category::Condition)
                .string(255),
            msi::Column::build("Ordering")
                .primary_key()
                .nullable()
                .range(0, 0x7fffffff)
                .int16(),
        ],
    )?;
    let mut rows = Vec::new();
    #[rustfmt::skip]
    let actions: [(&str, &str, &str, &str, &str, i32); 20] = [
        ("WelcomeDialog", "WelcomeCancel", "SpawnDialog", "CancelDialog", "1", 0),
        ("WelcomeDialog", "WelcomeInstall", "[Mode]", "Install", "1", 1),
        ("WelcomeDialog", "WelcomeInstall", "[Text_action]", "installation", "1", 2),
        ("WelcomeDialog", "WelcomeInstall", "[Text_agent]", "installer", "1", 3),
        ("WelcomeDialog", "WelcomeInstall", "[Text_Doing]", "Installing", "1", 4),
        ("WelcomeDialog", "WelcomeInstall", "[Text_done]", "installed", "1", 5),
        ("WelcomeDialog", "WelcomeInstall", "EndDialog", "Return", "1", 6),
        ("RemoveDialog", "RemoveCancel", "[Text_action]", "removal", "1", 7),
        ("RemoveDialog", "RemoveCancel", "SpawnDialog", "CancelDialog", "1", 8),
        ("RemoveDialog", "RemoveRemove", "[Mode]", "Remove", "1", 9),
        ("RemoveDialog", "RemoveRemove", "[Text_action]", "removal", "1", 10),
        ("RemoveDialog", "RemoveRemove", "[Text_agent]", "uninstaller", "1", 11),
        ("RemoveDialog", "RemoveRemove", "[Text_Doing]", "Removing", "1", 12),
        ("RemoveDialog", "RemoveRemove", "[Text_done]", "uninstalled", "1", 13),
        ("RemoveDialog", "RemoveRemove", "EndDialog", "Return", "1", 14),
        ("CancelDialog", "CancelNo", "EndDialog", "Return", "1", 15),
        ("CancelDialog", "CancelYes", "EndDialog", "Exit", "1", 16),
        ("ProgressDialog", "ProgressCancel", "SpawnDialog", "CancelDialog", "1", 17),
        ("ExitDialog", "ExitFinish", "EndDialog", "Return", "1", 18),
        ("FatalErrorDialog", "FatalFinish", "EndDialog", "Exit", "1", 19),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            msi::Value::Str(action.1.to_string()),
            msi::Value::Str(action.2.to_string()),
            msi::Value::Str(action.3.to_string()),
            msi::Value::Str(action.4.to_string()),
            msi::Value::Int(action.5),
        ]);
    }
    package.insert_rows(msi::Insert::into("ControlEvent").rows(rows))?;
    Ok(())
}

fn create_event_mapping_table(
    package: &mut Package,
    _cabinets: &[CabinetInfo],
) -> crate::Result<()> {
    package.create_table(
        "EventMapping",
        vec![
            msi::Column::build("Dialog_").id_string(72),
            msi::Column::build("Control_")
                .primary_key()
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Event")
                .category(msi::Category::Identifier)
                .string(50),
            msi::Column::build("Attribute")
                .category(msi::Category::Identifier)
                .string(50),
        ],
    )?;
    let mut rows = Vec::new();
    #[rustfmt::skip]
    let actions: [(&str, &str, &str, &str); 2] = [
        ("ProgressDialog", "ProgressActionText", "ActionText", "Text"),
        ("ProgressDialog", "ProgressProgressBar", "SetProgress", "Progress"),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            msi::Value::Str(action.1.to_string()),
            msi::Value::Str(action.2.to_string()),
            msi::Value::Str(action.3.to_string()),
        ]);
    }
    package.insert_rows(msi::Insert::into("EventMapping").rows(rows))?;
    Ok(())
}

fn create_text_style_table(package: &mut Package, _cabinets: &[CabinetInfo]) -> crate::Result<()> {
    package.create_table(
        "TextStyle",
        vec![
            msi::Column::build("TextStyle").primary_key().id_string(72),
            msi::Column::build("FaceName")
                .category(msi::Category::Text)
                .string(32),
            msi::Column::build("Size").range(0, 0x7fff).int16(),
            msi::Column::build("Color")
                .nullable()
                .range(0, 0xffffff)
                .int32(),
            msi::Column::build("StyleBits")
                .nullable()
                .range(0, 15)
                .int16(),
        ],
    )?;
    let mut rows = Vec::new();
    let actions: [(&str, &str, i32, i32, i32); 3] = [
        ("DefaultFont", "Tahoma", 10, 0, 0),
        ("BoldFont", "Tahoma", 10, 0, 1),
        ("TitleFont", "Verdana", 14, 0, 1),
    ];
    for action in actions {
        rows.push(vec![
            msi::Value::Str(action.0.to_string()),
            msi::Value::Str(action.1.to_string()),
            msi::Value::Int(action.2),
            msi::Value::Int(action.3),
            msi::Value::Int(action.4),
        ]);
    }
    package.insert_rows(msi::Insert::into("TextStyle").rows(rows))?;
    Ok(())
}

fn create_app_icon<W: Write>(writer: &mut W, settings: &Settings) -> crate::Result<()> {
    // Prefer ICO files.
    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("ico")) {
            io::copy(&mut fs::File::open(icon_path)?, writer)?;
            return Ok(());
        }
    }
    // TODO: Convert from other formats.
    Ok(())
}
