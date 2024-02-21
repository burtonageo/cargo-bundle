use crate::{
    bundle::{common, Settings},
    ResultExt,
};
use libflate::gzip;
use std::fs::create_dir_all;
use std::fs::{self, File};
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Builder;

// The YAML to make a development build
// Might eventually just use YML with a .replace() to add --release
const YML_DEV: &str = "
app-id: {id}
runtime: org.freedesktop.Platform
runtime-version: {runtime}
sdk: org.freedesktop.Sdk
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
{modules}
  - name: {name}
    buildsystem: simple
    build-commands:
      - cargo build --offline
      - make install
    sources:
      - type: archive
        path: {archive_path}

cleanup:
- '/target'
";

// YAML for release
const YML: &str = "
app-id: {id}
command: {id}
runtime: org.freedesktop.Platform
runtime-version: {runtime}
sdk: org.freedesktop.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
{permissions}
build-options:
  append-path: /usr/lib/sdk/rust-stable/bin
  env:
      CARGO_HOME: /run/build/{name}/cargo
      RUSTFLAGS: --remap-path-prefix =../
      RUST_BACKTRACE: \"1\"
modules:
{modules}
  - name: {name}
    buildsystem: simple
    build-commands:
      - cargo build --release --offline
      - make install
    sources:
        - type: archive
          path: {archive_path}
";

// The Makefile to install everything into the Flatpak
// Just would be used if it came with an extension or runtime
const MAKE: &str = "
.RECIPEPREFIX = *
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
const XML: &str = "
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
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
    common::print_warning("Flatpak bundle support is still expiremental.\nMake sure flatpak and flatpak-builder is installed")?;
    match settings.binary_arch() {
        "x86" => common::print_warning("Not all runtimes on Flathub support i386")?,
        "x86_64" => (),
        "aarch64" => (),
        "arm" => common::print_warning("Flathub does not support 32-bit ARM")?,
        _ => common::print_warning("Flathub may not support your architecture")?,
    };

    let mut dev = true;
    if settings.build_profile() != "dev" {
        dev = false;
    }

    // Having an option to only generate the necassary files might be useful
    // It could easily be removed if it isn't needed
    let generate_only = false;

    // Most of the formats name the bundle like this
    // deb_bundle.rs is the exception
    let bundling = format!("{}.flatpak", settings.bundle_name());

    if generate_only {
        common::print_bundling("Flatpak data")?;
    } else {
        common::print_bundling(&bundling)?;
    }

    if generate_only {
        generate(settings)?;
        Ok(vec![settings
            .project_out_directory()
            .join("bundle/flatpak/data")])
    } else if dev {
        generate(settings)?;
        flatpak(true, settings)?;
        Ok(vec![settings
            .project_out_directory()
            .join("bundle/flatpak")
            .join(&bundling)])
    } else {
        generate(settings)?;
        flatpak(false, settings)?;
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
    create_flatpak_yml(&data_dir, YML_DEV, Some(".dev"), settings)
        .chain_err(|| "Unable to create flatpak yml")?;
    create_flatpak_yml(&data_dir, YML, None, settings)
        .chain_err(|| "Unable to create flatpak yml")?;

    Ok(())
}

fn create_icons(settings: &Settings) -> crate::Result<Option<()>> {
    if settings.icon_files().count() == 0 {
        return Ok(None);
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

    Ok(())
}

fn create_src_archive(settings: &Settings) -> crate::Result<()> {
    Command::new("cargo").args(["vendor"]).output().ok();

    if !Path::new(".cargo/config.toml").exists() {
        let cargo = &mut common::create_file(Path::new(".cargo/config.toml"))
            .chain_err(|| "Failed to make tmp file")?;
        write!(cargo, "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"deps\"")?;
    } else if !config_check()?.0 {
        let mut cargo_config = File::options().append(true).open(".cargo/config.toml")?;
        cargo_config.write_all("\n[sources.crates-io]\nreplace-with = \"vendored-sources\"\n\n[sources.vendored-sources]\ndirectory = \"deps\"".as_bytes())?;
    } else if !config_check()?.1 {
        common::print_warning(
            "Some vendoring options in .cargo/config.toml can mess with the bundling process",
        )?;
    }

    let file = File::create("/tmp/placeholder.tar").chain_err(|| "Failed to create archive")?;
    let mut tarfile = Builder::new(file);
    tarfile
        .append_dir_all("vendor/src", "src")
        .chain_err(|| "src directory couldn't be put in archive")?;
    tarfile.append_dir_all("vendor/deps", "vendor").ok();
    tarfile
        .append_path_with_name("Cargo.toml", "vendor/Cargo.toml")
        .chain_err(|| "Cargo.toml couldn't be put in archive")?;
    tarfile
        .append_path_with_name("Cargo.lock", "vendor/Cargo.lock")
        .chain_err(|| "Cargo.lock couldn't be put in archive")?;
    tarfile
        .append_dir_all("vendor/.cargo", ".cargo")
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

    tarfile
        .append_path_with_name(xml_current, xml_path)
        .chain_err(|| "XML file couldn't be put in archive")?;
    tarfile
        .append_path_with_name(desktop_current, desktop_path)
        .chain_err(|| ".desktop file couldn't be put in archive")?;
    tarfile
        .append_path_with_name(makefile_current, "vendor/Makefile")
        .chain_err(|| "Makefile coud not be put in archive")?;
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

fn create_flatpak_yml(
    path: &Path,
    template: &str,
    infix: Option<&str>,
    settings: &Settings,
) -> crate::Result<()> {
    let mut path = PathBuf::from(path);
    path.push(format!(
        "{}{}.yml",
        settings.bundle_identifier(),
        infix.unwrap_or("")
    ));

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

    file.write_all(
        template
            .replace("{name}", settings.bundle_name())
            .replace("{id}", settings.bundle_identifier())
            .replace("{permissions}", &permission_list)
            .replace(
                "{runtime}",
                &settings
                    .runtime()
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "'23.08'".to_string()),
            )
            .replace(
                "{modules}",
                &settings
                    .modules()
                    .as_ref()
                    .map(|modules| modules.join("\n"))
                    .unwrap_or_default(),
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

fn flatpak(dev: bool, settings: &Settings) -> crate::Result<()> {
    let flatpak_build_rel = settings.project_out_directory().join("bundle/flatpak/");

    let mut manifest = format!("data/{}.yml", settings.bundle_identifier());
    if dev {
        manifest = format!("data/{}.dev.yml", settings.bundle_identifier());
    }

    let mut flatpak_builder = Command::new("flatpak-builder");
    flatpak_builder.current_dir(&flatpak_build_rel);
    flatpak_builder.arg("--install-deps-from=flathub");
    flatpak_builder.arg(format!("--repo={}repo", flatpak_build_rel.display()));
    flatpak_builder.arg("--force-clean");
    flatpak_builder.arg(format!("--state-dir={}/state", flatpak_build_rel.display()));
    flatpak_builder.arg(format!(
        "{}{}",
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
        .arg(format!("{}repo", flatpak_build_rel.display()))
        .arg(&flatpak_file_name)
        .arg(settings.bundle_identifier())
        .spawn()?;
    flatpak_bundler.wait()?;
    Ok(())
}

fn config_check() -> crate::Result<(bool, bool)> {
    let mut config = File::open(".cargo/config.toml")?;
    let mut contents = String::new();

    let check = r#"[sources.crates-io]
replace-with = "vendored-sources"

[sources.vendored-sources]
directory = "deps""#;

    config.read_to_string(&mut contents)?;
    Ok((
        contents.contains("[sources.crates-io]"),
        contents.contains(check),
    ))
}
