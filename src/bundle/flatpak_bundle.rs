use tar::Builder;
use super::common;
use libflate::gzip;
use chrono;
use dirs;
use icns;
use process::Command;
use std::cmp::min;
use std::ffi::OsStr;
use std::fs::create_dir_all;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
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
      - cargo build --release --offline
      - make -C install
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
      - make -C install
    sources:
        - type: archive
          path: {archive_path}
";

const MAKE: &str = "
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
    let dev = true;
    let generate_only = false;
    
    if generate_only {
        generate(settings)?;
        return Ok(vec![PathBuf::from(settings.project_out_directory())]);
    } else if dev {
        generate(settings)?;
        flatpak(true, settings)?;
        return Ok(vec![PathBuf::from(settings.project_out_directory())]);
    } else {
        generate(settings)?;
        flatpak(false, settings)?;
        return Ok(vec![PathBuf::from(settings.project_out_directory())]);
    }
}

fn generate(settings: &Settings) -> ::Result<()> {
    let gen_path = settings.project_out_directory().join("bundle/flatpak");
    if gen_path.exists() {
        fs::remove_dir_all(&gen_path).chain_err(|| format!("Failed to remove old flatpak files"))?;
    }
    fs::create_dir_all(&gen_path).chain_err(|| format!("Failed to create bundle directory at {:?}", gen_path))?;

    let data_dir = gen_path.join("data");
    create_dir_all(&data_dir).expect("Could not create data build directory.");
    create_src_archive(settings);
    create_desktop_file(settings, &data_dir).expect("Could not create desktop file");
    create_flatpak_yml(&data_dir, YML_DEV, Some(".dev"), settings).expect("Unable to create flatpak yml");
    create_flatpak_yml(&data_dir, YML, None, settings).expect("Unable to create flatpak yml");

    create_app_xml(settings);
    Ok(())
}

fn create_src_archive(settings: &Settings) -> ::Result<()> {
    let mut vendor = Command::new("cargo").args(["vendor"]).spawn().expect("vendoring failed").wait().expect("vendoring failed");
    
    if !Path::new(".cargo/config.toml").exists() {
        let cargo = &mut common::create_file(Path::new(".cargo/config.toml")).expect("Failed to make tmp file");
        write!(cargo, "[sources.crates-io]\nreplace-with = \"vendored-sources\"\n\n[sources.vendored-sources]\ndirectory = \"deps\"")?;
    } else {
        let mut cargo_config = File::options().append(true).open(".cargo/config.toml")?;
        cargo_config.write_all("\n[sources.crates-io]\nreplace-with = \"vendored-sources\"\n\n[sources.vendored-sources]\ndirectory = \"deps\"".as_bytes())?;
    }

    let file = File::create("/tmp/placeholder.tar").expect("Failed to create archive");
    let mut tarfile = Builder::new(file);
    tarfile.append_dir_all("vendor/src", "src").expect("src directory couldn't be put in archive");
    tarfile.append_dir_all("vendor/deps", "vendor").expect("vendor directory couldn't be archived");
    tarfile.append_path_with_name("Cargo.toml", "vendor/Cargo.toml").expect("Cargo.toml couldn't be put in archive");
    tarfile.append_path_with_name("Cargo.lock", "vendor/Cargo.lock").expect("Cargo.lock couldn't be put in archive");
    tarfile.append_dir_all("vendor/.cargo", ".cargo").expect("Couldn't add file to archive");
    for src in settings.resource_files() {
        let src = src?;
        tarfile.append_path(src);
    }

    let mut input = File::open("/tmp/placeholder.tar").expect("Not able to open file");
    let mut path = settings
        .project_out_directory()
        .join("bundle/flatpak/data/")
        .join(format!("{}{}.tar.gz", settings.bundle_identifier(), settings.version_string()));

    let output = Box::new(fs::File::create(path).expect(&format!("Can't create file: {}{}.tar.gz", settings.bundle_identifier(), settings.version_string())));
    let mut output = io::BufWriter::new(output);
    let mut encoder = gzip::Encoder::new(output).expect("Failed to encode");
    io::copy(&mut input, &mut encoder).expect("Encoding GZIP stream failed");
    encoder.finish().into_result().unwrap();

    std::fs::remove_file("/tmp/placeholder.tar").expect("placeholder.tar deletion failed");
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
    write!(file, "\nCategories={:?}", settings.app_category().unwrap())?;
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
                &format!("{}{}.tar.gz", &settings.bundle_identifier(), &settings.version_string())
            )
            .as_bytes(),
    )?;
    Ok(())
}

fn create_app_xml(settings: &Settings) -> ::Result<()> {
    let mut path = settings
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

fn flatpak(release: bool, settings: &Settings) -> ::Result<()> {
    let flatpak_build_rel = settings.project_out_directory().join("bundle/flatpak/");
    
    let mut c = Command::new("flatpak-builder");
    c.current_dir(&flatpak_build_rel);
    if release {
        c.arg(format!("--repo={}repo", flatpak_build_rel.display()));
        c.arg("--force-clean");
        c.arg(format!("--state-dir={}/state", flatpak_build_rel.display()));
        c.arg(format!("{}{}", flatpak_build_rel.display(), settings.binary_arch()));
        c.arg(format!("data/{}.dev.yml", settings.bundle_identifier()));
    
        let mut c = c.spawn()?;
        c.wait()?;

        let flatpak_file_name = format!("{}.flatpak", settings.bundle_name());

        let mut c2 = Command::new("flatpak")
            .current_dir(&flatpak_build_rel)
            .arg("build-bundle")
            .arg(format!("{}repo", flatpak_build_rel.display()))
            .arg(format!("../{}", flatpak_file_name))
            .arg(settings.bundle_identifier())
            .spawn()?;
        c2.wait()?;
    }
    Ok(())
}
