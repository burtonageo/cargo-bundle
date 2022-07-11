use super::common;
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

pub fn bundle_project(settings: &Settings) -> ::Result<Vec<PathBuf>> {
    if true {
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
    Ok(())
}

fn create_app_xml() {}

fn flatpak(release: bool, settings: &Settings) -> ::Result<()> {
    let flatpak_build_rel = settings.project_out_directory().join("bundle/flatpak/repo");
    let flatpak_temp = match prepare_flatpak_temp(settings.project_out_directory()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Could not prepare flatpak-temp");
            std::process::exit(-1);
        }
    };

    let mut c = Command::new("flatpak-builder");
    c.current_dir(&flatpak_temp);
    if release {
        c.arg(format!("--repo={}", flatpak_build_rel.display()));
        c.arg("--force-clean");
        c.arg(format!("--state-dir={}/../state", flatpak_build_rel.display()));
        c.arg(format!("{}/../{}", flatpak_build_rel.display(), settings.binary_arch()));
        c.arg(format!("../data/{}.dev.yml", settings.bundle_identifier()));
    }
    let mut c = c.spawn()?;
    c.wait()?;

    let flatpak_file_name = format!("{}.flatpak", settings.bundle_name());

    let mut c2 = Command::new("flatpak")
        .current_dir(&flatpak_temp)
        .arg("build-bundle")
        .arg(format!("{}/repo", flatpak_build_rel.display()))
        .arg(format!("../{}", flatpak_file_name))
        .arg(settings.bundle_identifier())
        .spawn()?;
    c2.wait()?;
    Ok(())
}

fn prepare_flatpak_temp(project_dir: &Path) -> ::Result<PathBuf> {
    let flatpak_temp = project_dir.join("target/bundle/flatpak/flatpak-temp");

    Ok(flatpak_temp)
}
