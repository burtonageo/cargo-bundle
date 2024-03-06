use crate::{
    bundle::{
        common,
        linux::common::{generate_icon_files_non_png, generate_icon_files_png},
        Settings,
    },
    ResultExt,
};
use libflate::gzip;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::create_dir_all;
use std::fs::{self, File};
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Builder;

// The YAML to make a development build
// Might eventually just use YML with a .replace() to add --release
const YML: &str = "app-id: {id}
runtime: {runtime}.Platform
runtime-version: {runtime_version}
sdk: {runtime}.Sdk
appstream-compose: false
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: {id}
{permissions}
build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  env:
      CARGO_HOME: /run/build/{name}/cargo
      RUSTFLAGS: --remap-path-prefix =../
      RUST_BACKTRACE: \"1\"
modules:
  - name: {name}
    buildsystem: simple
    build-commands:
      - {cargo_build} 
      - make install
    sources:
      - type: archive
        path: {archive_path}

cleanup:
- '/target'
";

// The Makefile to install everything into the Flatpak
// Just would be used if it came with an extension or runtime
const MAKE: &str = ".RECIPEPREFIX = *
BASE_DIR=$(realpath .)

RELEASE={RELEASE}

BIN_NAME={BIN_NAME}
ROOT=/app
BIN_DIR=$(ROOT)/bin
SHARE_DIR=$(ROOT)/share
TARGET_DIR=$(BASE_DIR)/target

install:
* @echo \"Installing binary into $(BIN_DIR)/{APP_ID}\"
* @strip \"$(TARGET_DIR)/$(RELEASE)/$(BIN_NAME)\"
* @mkdir -p $(BIN_DIR)
* @install \"$(TARGET_DIR)/$(RELEASE)/$(BIN_NAME)\" \"$(BIN_DIR)/{APP_ID}\"

* @echo Installing icons into $(SHARE_DIR)/icons/hicolor
* @ls icons
* @ls icons/hicolor
* @install -D icons/hicolor/* -t $(SHARE_DIR)/icons/hicolor
* @# Force cache of icons to refresh
* @mkdir -p $(SHARE_DIR)/applications/
* @mkdir -p $(SHARE_DIR)/metainfo/

* @echo Installing .desktop and .xml into $(SHARE_DIR)/applications, $(SHARE_DIR)/metainfo
* @install -m 644 {APP_ID}.appdata.xml $(SHARE_DIR)/metainfo/{APP_ID}.appdata.xml
* @install -m 644 {APP_ID}.desktop $(SHARE_DIR)/applications/{APP_ID}.desktop
*
* @echo Installing resource files
* @mkdir -p ~/.var/app/{APP_ID}/data
* @ls ~/.var/app
* @install -m 644 resources/$(BIN_NAME)/* ~/.var/app/{APP_ID}/data/
";

// Some metadata for the generated Flatpak
const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<component type=\"desktop\">
    <id>{id}</id>
    <name>{name}</name>
    {license}
    {metadata_license}
    <developer_name>{author}</developer_name>
    <summary>{summary}</summary>
    <url type=\"homepage\">{homepage}</url>
    <url type=\"bugtracker\">{repository}</url>
    <description>
        <p>{description}</p>
    </description>
    <launchable type=\"desktop-id\">{id}.desktop</launchable>
    <provides>
        <binary>{id}</binary>
    </provides>
   <translation type=\"gettext\">{name}</translation>
</component>";

pub fn bundle_project(settings: &Settings) -> crate::Result<Vec<PathBuf>> {
    common::print_warning("Make sure flatpak and flatpak-builder is installed")?;
    match settings.binary_arch() {
        "x86" => common::print_warning("Not all runtimes on Flathub support i386")?,
        "x86_64" => (),
        "aarch64" => (),
        "arm" => common::print_warning("Flathub does not support 32-bit ARM")?,
        _ => common::print_warning("Flathub may not support your architecture")?,
    };

    // Most of the formats name the bundle like this
    // deb_bundle.rs is the exception
    let bundling = format!("{}.flatpak", settings.bundle_name());

    if settings.generate_only() {
        common::print_bundling("Flatpak data")?;
    } else {
        common::print_bundling(&bundling)?;
    }

    if settings.generate_only() {
        generate(settings)?;
        Ok(vec![settings
            .project_out_directory()
            .join("bundle/flatpak/data")])
    } else {
        generate(settings)?;
        flatpak(settings)?;
        Ok(vec![settings
            .project_out_directory()
            .join("bundle/flatpak")
            .join(&bundling)])
    }
}

fn generate(settings: &Settings) -> crate::Result<()> {
    let gen_path = settings.project_out_directory().join("bundle/flatpak");
    if gen_path.exists() {
        fs::remove_dir_all(&gen_path)
            .chain_err(|| "Failed to remove old flatpak files".to_string())?;
    }
    fs::create_dir_all(&gen_path)
        .chain_err(|| format!("Failed to create bundle directory at {:?}", gen_path))?;

    let data_dir = gen_path.join("data");
    create_dir_all(&data_dir).chain_err(|| "Could not create data build directory.")?;
    create_desktop_file(settings, &data_dir).chain_err(|| "Could not create desktop file")?;
    create_makefile(settings).chain_err(|| "Could not create Makefile")?;
    create_app_xml(settings).chain_err(|| "Unable to create XML")?;
    create_icons(settings).chain_err(|| "Unable to create icons")?;
    create_src_archive(settings)?;
    create_flatpak_yml(&data_dir, settings).chain_err(|| "Unable to create flatpak yml")?;

    Ok(())
}

fn create_icons(settings: &Settings) -> crate::Result<Option<()>> {
    let base_dir = settings
        .project_out_directory()
        .join("bundle/flatpak/data/icons/hicolor");
    if settings.icon_files().count() == 0 {
        if !base_dir.exists() {
            std::fs::create_dir_all(base_dir.clone())?;
            let mut ignore = std::fs::File::create(base_dir.join("ignoreme"))?;
            ignore.write_all(b"ignore me")?;
        };
        return Ok(None);
    }

    let mut sizes: BTreeSet<(u32, u32, bool)> = BTreeSet::new();

    for icon_path in settings.icon_files() {
        let icon_path = icon_path?;
        if icon_path.extension() == Some(OsStr::new("png")) {
            let new_sizes = generate_icon_files_png(
                &icon_path,
                &base_dir,
                settings.binary_name(),
                sizes.clone(),
            )
            .unwrap();
            sizes.append(&mut new_sizes.to_owned())
        } else {
            let new_sizes = generate_icon_files_non_png(
                &icon_path,
                &base_dir,
                settings.binary_name(),
                sizes.clone(),
            )
            .unwrap();
            sizes.append(&mut new_sizes.to_owned())
        }
    }
    Ok(Some(()))
}

fn create_makefile(settings: &Settings) -> crate::Result<()> {
    let path = settings
        .project_out_directory()
        .join("bundle/flatpak/data/Makefile");

    let mut file = File::create(path)?;

    let mut profile = settings.build_profile();
    if settings.build_profile() == "dev" {
        profile = "debug";
    }

    let template = MAKE;
    file.write_all(
        template
            .replace("{APP_ID}", settings.bundle_identifier())
            .replace("{BIN_NAME}", settings.binary_name())
            .replace("{RELEASE}", profile)
            .as_bytes(),
    )?;

    file.flush()?;

    Ok(())
}

fn create_src_archive(settings: &Settings) -> crate::Result<()> {
    Command::new("cargo").args(["vendor"]).output().ok();

    let cargo_config = &mut common::create_file(Path::new("/tmp/config.toml"))
        .chain_err(|| "Failed to make tmp file")?;

    if config_check()?.1 {
        let old_config = fs::read_to_string(".cargo/config.toml")?;
        cargo_config.write_all(old_config.as_bytes())?;
    } else if config_check()?.0 {
        common::print_warning(
            "Some vendoring options in .cargo/config.toml can mess with the bundling process",
        )?;
    } else if Path::new(".cargo/config.toml").exists() {
        let old_config = fs::read_to_string(".cargo/config.toml")?;
        cargo_config.write_all(format!("{old_config}\n[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"").as_bytes())?;
    } else {
        cargo_config.write_all("[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"".as_bytes())?;
    }

    cargo_config.flush()?;

    let file = File::create("/tmp/placeholder.tar").chain_err(|| "Failed to create archive")?;
    let mut tarfile = Builder::new(file);
    tarfile
        .append_dir_all("vendor/src", "src")
        .chain_err(|| "src directory couldn't be put in archive")?;
    tarfile.append_dir_all("vendor/vendor", "vendor").ok();
    tarfile
        .append_path_with_name("Cargo.toml", "vendor/Cargo.toml")
        .chain_err(|| "Cargo.toml couldn't be put in archive")?;
    tarfile
        .append_path_with_name("Cargo.lock", "vendor/Cargo.lock")
        .chain_err(|| "Cargo.lock couldn't be put in archive")?;
    tarfile
        .append_path_with_name("/tmp/config.toml", "vendor/.cargo/config.toml")
        .chain_err(|| "Couldn't add file to archive")?;

    let mut state = 0;
    for src in settings.resource_files() {
        state += 1;
        let src = src?;
        let dest = Path::new("vendor/resources")
            .join(settings.binary_name())
            .join(common::resource_relpath(&src));
        tarfile
            .append_path_with_name(&src, &dest)
            .chain_err(|| "Failed to copy resources to bundle")?;
    }
    // Got to have something in the location, else the Makefile will fail
    if state == 0 {
        let dest = Path::new("vendor/resources")
            .join(settings.binary_name())
            .join("file");
        tarfile
            .append_path_with_name("Cargo.toml", dest)
            .chain_err(|| "Failed to copy resource to bundle")?;
    }

    let xml = format!("{}.appdata.xml", settings.bundle_identifier());
    let desktop = format!("{}.desktop", settings.bundle_identifier());
    let out_dir = settings.project_out_directory().join("bundle/flatpak/data");
    let xml_path = format!("vendor/{}", xml);
    let desktop_path = format!("vendor/{}", desktop);
    let desktop_current = format!("{}/{}", out_dir.to_str().unwrap(), desktop);
    let xml_current = format!("{}/{}", out_dir.to_str().unwrap(), xml);
    let makefile_current = format!("{}/Makefile", out_dir.to_str().unwrap());
    let icons_current = format!("{}/icons", out_dir.to_str().unwrap());

    tarfile
        .append_path_with_name(xml_current, xml_path)
        .chain_err(|| "XML file couldn't be put in archive")?;
    tarfile
        .append_path_with_name(desktop_current, desktop_path)
        .chain_err(|| ".desktop file couldn't be put in archive")?;
    tarfile
        .append_path_with_name(makefile_current, "vendor/Makefile")
        .chain_err(|| "Makefile coud not be put in archive")?;
    tarfile
        .append_dir_all("vendor/icons", icons_current)
        .chain_err(|| "failed to include icons")?;
    for src in settings.resource_files() {
        let src = src?;
        tarfile
            .append_path(src)
            .chain_err(|| "Couldn't add resources")?;
    }

    let mut input =
        File::open("/tmp/placeholder.tar").chain_err(|| "Not able to open file".to_string())?;
    let path = settings
        .project_out_directory()
        .join("bundle/flatpak/data/")
        .join(format!(
            "{}.{}.tar.gz",
            settings.bundle_identifier(),
            settings.version_string()
        ));

    let output = Box::new(fs::File::create(path).chain_err(|| {
        format!(
            "Can't create file: {}.{}.tar.gz",
            settings.bundle_identifier(),
            settings.version_string()
        )
    })?);
    let output = io::BufWriter::new(output);
    let mut encoder = gzip::Encoder::new(output).chain_err(|| "Failed to encode".to_string())?;
    io::copy(&mut input, &mut encoder).chain_err(|| "Encoding GZIP stream failed".to_string())?;
    encoder
        .finish()
        .into_result()
        .chain_err(|| "Encoding GZIP failed".to_string())?;

    std::fs::remove_file("/tmp/placeholder.tar")
        .chain_err(|| "placeholder.tar deletion failed".to_string())?;
    Ok(())
}

// It was easier to have it like this, rather than have a template
fn create_desktop_file(settings: &Settings, path: &Path) -> crate::Result<()> {
    let mut path = PathBuf::from(path);
    path.push(format!("{}.desktop", settings.bundle_identifier()));

    let mut file = File::create(path)?;

    write!(file, "[Desktop Entry]\nName={}", settings.bundle_name())?;
    write!(
        file,
        "\nGenericName={}\nComment={}",
        settings.bundle_name(),
        settings.short_description()
    )?;
    if let Some(category) = settings.app_category() {
        write!(
            file,
            "\nCategories={:?}",
            category.gnome_desktop_categories(),
        )?;
    }
    write!(file, "\nExec={}", settings.bundle_identifier())?;
    //    write!(
    //        file,
    //        "\nIcon{}",
    //        settings.bundle_identifier()
    //    )?;
    write!(
        file,
        "\nTerminal=false\nType=Application\nStartupNotify=true"
    )?;
    write!(file, "\nX-Purism-FormFactor={}", settings.bundle_name())?; // Form factor should be put here
    Ok(())
}

fn create_flatpak_yml(path: &Path, settings: &Settings) -> crate::Result<()> {
    let template = YML;
    let mut path = PathBuf::from(path);
    path.push(format!("{}.yml", settings.bundle_identifier()));

    let mut file = File::create(path)?;

    let permissions = settings
        .permissions()
        .iter()
        .map(|p| format!("- --{}", p))
        .collect::<Vec<String>>()
        .join("\n  ");

    let mut permission_list = String::default();
    if permissions != String::default() {
        permission_list.push_str("finish-args: \n  ");
        permission_list.push_str(&permissions);
    }

    let mut cargo_build = "cargo build --offline".to_string();
    match settings.build_profile() {
        "dev" => {}
        "release" => {
            cargo_build.push_str("--release");
        }
        custom => {
            cargo_build.push_str("--profile");
            cargo_build.push_str(custom);
        }
    }
    if let Some(triple) = settings.target_triple() {
        cargo_build.push_str(&format!("--target={triple}"));
    }
    if let Some(features) = settings.features() {
        cargo_build.push_str(&format!("--features={features}"));
    }
    if settings.all_features() {
        cargo_build.push_str("--all-features");
    }

    file.write_all(
        template
            .replace("{cargo_build}", &cargo_build)
            .replace("{name}", settings.bundle_name())
            .replace("{id}", settings.bundle_identifier())
            .replace("{permissions}", &permission_list)
            .replace(
                "{runtime}",
                &settings
                    .runtime()
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "org.freedesktop".to_string()),
            )
            .replace(
                "{runtime_version}",
                &settings
                    .runtime_version()
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "'23.08'".to_string()),
            )
            .replace(
                "{archive_path}",
                &format!(
                    "{}.{}.tar.gz",
                    &settings.bundle_identifier(),
                    &settings.version_string()
                ),
            )
            .as_bytes(),
    )?;
    Ok(())
}

fn create_app_xml(settings: &Settings) -> crate::Result<()> {
    let path = settings
        .project_out_directory()
        .join("bundle/flatpak/data/")
        .join(format!("{}.appdata.xml", settings.bundle_identifier()));

    let mut file = File::create(path)?;

    let template = XML;

    let proj_license = match settings.copyright_string().unwrap_or("") {
        "" => "<project_license>LicenseRef-proprietary</project_license>".to_string(),
        _ => format!(
            "<project_license>{}</project_license>",
            settings.copyright_string().unwrap_or("")
        ),
    };

    let meta_license = match settings.copyright_string().unwrap_or("") {
        "" => "<metadata_license>CC0-1.0</metadata_license>".to_string(),
        _ => format!(
            "<metadata_license>{}</metadata_license>",
            settings.copyright_string().unwrap_or("")
        ),
    };

    file.write_all(
        template
            .replace("{id}", settings.bundle_identifier())
            .replace("{name}", settings.bundle_name())
            .replace("{summary}", settings.short_description())
            .replace(
                "{description}",
                settings
                    .long_description()
                    .unwrap_or("Flatpak Written in Rust"),
            )
            .replace("{license}", &proj_license)
            .replace("{homepage}", settings.homepage_url())
            .replace("{repository}", settings.homepage_url())
            .replace("{metadata_license}", &meta_license)
            .as_bytes(),
    )?;

    Ok(())
}

fn flatpak(settings: &Settings) -> crate::Result<()> {
    let flatpak_build_rel = settings.project_out_directory().join("bundle/flatpak");

    let manifest = format!("data/{}.yml", settings.bundle_identifier());

    let mut flatpak_builder = Command::new("flatpak-builder");
    flatpak_builder.current_dir(&flatpak_build_rel);
    flatpak_builder.arg("--install-deps-from=flathub");
    flatpak_builder.arg(format!("--repo={}/repo", flatpak_build_rel.display()));
    flatpak_builder.arg("--force-clean");
    flatpak_builder.arg(format!("--state-dir={}/state", flatpak_build_rel.display()));
    flatpak_builder.arg(format!(
        "{}/{}",
        flatpak_build_rel.display(),
        settings.binary_arch()
    ));
    flatpak_builder.arg(manifest);

    let mut flatpak_builder = flatpak_builder.spawn()?;
    flatpak_builder.wait()?;

    let flatpak_file_name = format!("{}.flatpak", settings.bundle_name());

    let mut flatpak_bundler = Command::new("flatpak")
        .current_dir(&flatpak_build_rel)
        .arg("build-bundle")
        .arg(format!("{}/repo", flatpak_build_rel.display()))
        .arg(&flatpak_file_name)
        .arg(settings.bundle_identifier())
        .spawn()?;
    flatpak_bundler.wait()?;
    Ok(())
}

fn config_check() -> crate::Result<(bool, bool)> {
    let config_path = Path::new(".cargo/config.toml");
    if !config_path.exists() {
        return Ok((false, false));
    }
    let mut config = File::open(config_path)?;
    let mut contents = String::new();

    let check = r#"[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor""#;

    config.read_to_string(&mut contents)?;
    Ok((
        contents.contains("[source.crates-io]"),
        contents.contains(check),
    ))
}
