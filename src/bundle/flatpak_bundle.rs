use super::common;
use libflate::gzip;
use std::fs::create_dir_all;
use std::fs::{self, File};
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Builder;
use {ResultExt, Settings};

const YML_DEV: &str = "
app-id: {id}
runtime: org.gnome.Platform
runtime-version: {runtime}
sdk: org.gnome.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: {id}
finish-args:
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

const YML: &str = "
app-id: {id}
command: {id}
runtime: org.gnome.Platform
runtime-version: {runtime}
sdk: org.gnome.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
finish-args:
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
* @install -m 644 {APP_ID}.appdata.xml $(SHARE_DIR)/metainfo/$(APP_ID).appdata.xml
* @install -m 644 {APP_ID}.desktop $(SHARE_DIR)/applications/{APP_ID}.desktop
";

const XML: &str = "
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<component type=\"desktop\">
    <id>{id}</id>
    <name>{name}</name>
    <project_license>{license}</project_license>
    <metadata_license>{metadata_license}</metadata_license>
    {author}
    <summary>{summary}</summary>
    <url type=\"homepage\">{homepage}</url>
    <url type=\"bugtracker\">{repository}</url>
    <description>
        {description}
    </description>
    <launchable type=\"desktop-id\">{id}.desktop</launchable>
    <provides>
        <binary>{id}</binary>
    </provides>
{categories}
{screenshots}
{releases}
{content_rating}
{recommends}
    <translation type=\"gettext\">{name}</translation>
</component>";

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    let mut dev = true;
    if !(settings.build_profile() == "dev") {
        dev = false;
    }
    let generate_only = false;

    if generate_only {
        generate(settings)?;
        return Ok(vec![PathBuf::from(
            settings.project_out_directory().join("bundle/flatpak"),
        )]);
    } else if dev {
        generate(settings)?;
        flatpak(true, settings)?;
        return Ok(vec![PathBuf::from(
            settings.project_out_directory().join("bundle/flatpak"),
        )]);
    } else {
        generate(settings)?;
        flatpak(false, settings)?;
        return Ok(vec![PathBuf::from(
            settings.project_out_directory().join("bundle/flatpak"),
        )]);
    }
}

fn generate(settings: &Settings) -> ::Result<()> {
    let gen_path = settings.project_out_directory().join("bundle/flatpak");
    if gen_path.exists() {
        fs::remove_dir_all(&gen_path).chain_err(|| format!("Failed to remove old flatpak files"))?;
    }
    fs::create_dir_all(&gen_path).chain_err(|| format!("Failed to create bundle directory at {:?}", gen_path))?;

    let data_dir = gen_path.join("data");
    create_dir_all(&data_dir).chain_err(|| "Could not create data build directory.")?;
    create_desktop_file(settings, &data_dir).chain_err(|| "Could not create desktop file")?;
    create_makefile(settings).chain_err(|| "Could not create Makefile")?;
    create_app_xml(settings).chain_err(|| "Unable to create XML")?;
    create_icons(settings).chain_err(|| "Unable to create icons")?;
    create_src_archive(settings)?;
    create_flatpak_yml(&data_dir, YML_DEV, Some(".dev"), settings).chain_err(|| "Unable to create flatpak yml")?;
    create_flatpak_yml(&data_dir, YML, None, settings).chain_err(|| "Unable to create flatpak yml")?;

    Ok(())
}

fn create_icons(settings: &Settings) -> ::Result<()> {
    Ok(())
}

fn create_makefile(settings: &Settings) -> ::Result<()> {
    let path = settings.project_out_directory().join("bundle/flatpak/data/Makefile");

    let mut file = File::create(path)?;

    let mut profile = settings.build_profile();
    if settings.build_profile() == "dev" {
        profile = "debug";
    }

    let template = MAKE;
    file.write(
        template
            .replace("{APP_ID}", settings.bundle_identifier())
            .replace("{BIN_NAME}", settings.binary_name())
            .replace("{RELEASE}", profile)
            .as_bytes(),
    )?;

    Ok(())
}

fn create_src_archive(settings: &Settings) -> ::Result<()> {
    Command::new("cargo").args(["vendor"]).output().ok();

    if !Path::new(".cargo/config.toml").exists() {
        let cargo =
            &mut common::create_file(Path::new(".cargo/config.toml")).chain_err(|| "Failed to make tmp file")?;
        write!(cargo, "[sources.crates-io]\nreplace-with = \"vendored-sources\"\n\n[sources.vendored-sources]\ndirectory = \"deps\"")?;
    } else if !config_check()? {
        let mut cargo_config = File::options().append(true).open(".cargo/config.toml")?;
        cargo_config.write_all("\n[sources.crates-io]\nreplace-with = \"vendored-sources\"\n\n[sources.vendored-sources]\ndirectory = \"deps\"".as_bytes())?;
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
        tarfile.append_path(src).chain_err(|| "Couldn't add resources")?;
    }

    let mut input = File::open("/tmp/placeholder.tar").chain_err(|| format!("Not able to open file"))?;
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
    let mut encoder = gzip::Encoder::new(output).chain_err(|| format!("Failed to encode"))?;
    io::copy(&mut input, &mut encoder).chain_err(|| format!("Encoding GZIP stream failed"))?;
    encoder
        .finish()
        .into_result()
        .chain_err(|| format!("Encoding GZIP failed"))?;

    std::fs::remove_file("/tmp/placeholder.tar").chain_err(|| format!("placeholder.tar deletion failed"))?;
    Ok(())
}

fn create_desktop_file(settings: &Settings, path: &Path) -> ::Result<()> {
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
    write!(
        file,
        "\nCategories={:?}",
        settings
            .app_category()
            .chain_err(|| format!("Failed to read categories"))?
    )?;
    write!(
        file,
        "\nIcon={}\nExec={}",
        settings.bundle_identifier(),
        settings.bundle_identifier()
    )?;
    write!(file, "\nTerminal=false\nType=Application\nStartupNotify=true")?;
    write!(file, "\nX-Purism-FormFactor={}", settings.bundle_name())?; // Form factor should be put here
    Ok(())
}

fn create_flatpak_yml(path: &Path, template: &str, infix: Option<&str>, settings: &Settings) -> ::Result<()> {
    let mut path = PathBuf::from(path);
    path.push(format!("{}{}.yml", settings.bundle_identifier(), infix.unwrap_or("")));

    let mut file = File::create(path)?;

    let permissions = &settings
        .permissions()
        .iter()
        .map(|p| format!("- --{}", p))
        .collect::<Vec<String>>()
        .join("\n  ");
    file.write_all(
        template
            .replace("{name}", settings.bundle_name())
            .replace("{id}", settings.bundle_identifier())
            .replace("{permissions}", &permissions)
            .replace(
                "{runtime}",
                &settings
                    .runtime()
                    .as_ref()
                    .map(|s| format!("\"{}\"", s))
                    .unwrap_or_else(|| "\"42\"".to_string()),
            )
            .replace(
                "{modules}",
                &settings
                    .modules()
                    .as_ref()
                    .map(|modules| modules.join("\n"))
                    .unwrap_or_else(|| "".to_string()),
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

fn create_app_xml(settings: &Settings) -> ::Result<()> {
    let path = settings
        .project_out_directory()
        .join("bundle/flatpak/data/")
        .join(format!("{}.appdata.xml", settings.bundle_identifier()));

    let mut file = File::create(path)?;

    let template = XML;
    file.write(
        template
            .replace("{id}", settings.bundle_identifier())
            .replace("{name}", settings.bundle_name())
            .replace("{summary}", settings.short_description())
            .replace("{description}", settings.long_description().unwrap_or(""))
            .replace("{license}", settings.copyright_string().unwrap_or(""))
            .replace("{homepage}", settings.homepage_url())
            .replace("{repository}", settings.homepage_url())
            .replace("{metadata_license}", settings.copyright_string().unwrap_or(""))
            .as_bytes(),
    )?;

    Ok(())
}

fn flatpak(dev: bool, settings: &Settings) -> ::Result<()> {
    let flatpak_build_rel = settings.project_out_directory().join("bundle/flatpak/");

    let mut manifest = format!("data/{}.yml", settings.bundle_identifier());
    if dev {
        manifest = format!("data/{}.dev.yml", settings.bundle_identifier());
    }

    let mut c = Command::new("flatpak-builder");
    c.current_dir(&flatpak_build_rel);
    c.arg(format!("--repo={}repo", flatpak_build_rel.display()));
    c.arg("--force-clean");
    c.arg(format!("--state-dir={}/state", flatpak_build_rel.display()));
    c.arg(format!("{}{}", flatpak_build_rel.display(), settings.binary_arch()));
    c.arg(manifest);

    let mut c = c.spawn()?;
    c.wait()?;

    let flatpak_file_name = format!("{}.flatpak", settings.bundle_name());

    let mut c2 = Command::new("flatpak")
        .current_dir(&flatpak_build_rel)
        .arg("build-bundle")
        .arg(format!("{}repo", flatpak_build_rel.display()))
        .arg(format!("{}", flatpak_file_name))
        .arg(settings.bundle_identifier())
        .spawn()?;
    c2.wait()?;
    Ok(())
}

fn config_check() -> ::Result<bool> {
    let mut config = File::open(".cargo/config.toml")?;
    let mut contents = String::new();
    config.read_to_string(&mut contents)?;
    Ok(contents.contains("[sources.crates-io]"))
}
