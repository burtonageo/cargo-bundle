use super::common;
use chrono;
use dirs;
use icns;
use std::cmp::min;
use std::ffi::OsStr;
use std::fs::create_dir_all;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use {ResultExt, Settings};

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    generate(settings)?;
    flatpak()?;
    Ok(vec![PathBuf::from(settings.project_out_directory())])
}

fn generate(settings: &Settings) -> ::Result<()> {
    let gen_path = settings.project_out_directory().join("bundle/flatpak");
    if gen_path.exists() {
        fs::remove_dir_all(&gen_path).chain_err(|| format!("Failed to remove old flatpak files"))?;
    }
    fs::create_dir_all(&gen_path).chain_err(|| format!("Failed to create bundle directory at {:?}", gen_path))?;

    let data_dir = gen_path.join("data");
    create_dir_all(&data_dir).expect("Could not create data build directory.");
    create_desktop_file(settings, &data_dir).expect("Could not create desktop file");
    create_flatpak_yml(&data_dir, "ignore this", None, settings).expect("Unable to create flatpak yml");
    create_flatpak_yml(&data_dir, "ignore this", None, settings).expect("Unable to create flatpak yml");

    create_app_xml();
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
    todo!()
}

fn create_app_xml() {
 todo!();
}

fn flatpak() -> ::Result<()> {
    Ok(())
}
