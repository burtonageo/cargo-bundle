extern crate ar;
#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate icns;
extern crate image;
extern crate libflate;
extern crate md5;
extern crate tar;
extern crate target_build_utils;
extern crate term;
extern crate toml;
extern crate walkdir;

mod bundle;

use bundle::{Settings, bundle_project};
use clap::{App, AppSettings, SubCommand};
use std::env;
use std::process;

error_chain! {
    foreign_links {
        Io(::std::io::Error);
        Image(::image::ImageError);
        Target(::target_build_utils::Error);
        Term(::term::Error);
        Walkdir(::walkdir::Error);
    }
    errors { }
}

/// Runs `cargo build` to make sure the binary file is up-to-date.
fn build_project_if_unbuilt(settings: &Settings) -> Result<()> {
    let mut args = vec!["build".to_string()];
    if let Some(triple) = settings.target_triple() {
        args.push(format!("--target={}", triple));
    }
    if settings.is_release_build() {
        args.push("--release".to_string());
    }
    let status = process::Command::new("cargo").args(args).status()?;
    if !status.success() {
        bail!("Result of `cargo build` operation was unsuccessful: {}",
              status);
    }
    Ok(())
}

quick_main!(run);

fn run() -> ::Result<()> {
    let m = App::new("cargo-bundle")
                .author("George Burton <burtonageo@gmail.com>")
                .about("Bundle rust executables into OS bundles")
                .version(format!("v{}", crate_version!()).as_str())
                .bin_name("cargo")
                .settings(&[AppSettings::GlobalVersion, AppSettings::SubcommandRequired])
                .subcommand(SubCommand::with_name("bundle").args_from_usage(
                    "-d --resources-directory [DIR] 'Directory which contains bundle resources (images, etc)'\n\
                     -r --release 'Build a bundle from a target built in release mode'\n\
                     --target [TRIPLE] 'Build a bundle for the target triple'\n\
                     -f --format [FORMAT] 'Which format to use for the bundle'"))
                .get_matches();

    if let Some(m) = m.subcommand_matches("bundle") {
        let output_paths = env::current_dir().map_err(From::from)
            .and_then(|d| Settings::new(d, m))
            .and_then(|s| {
                          try!(build_project_if_unbuilt(&s));
                          Ok(s)
                      })
            .and_then(bundle_project)?;
        let pluralised = if output_paths.len() == 1 {
            "bundle"
        } else {
            "bundles"
        };
        println!("{} {} created at:", output_paths.len(), pluralised);
        for bundle in output_paths {
            println!("\t{}", bundle.display());
        }
    }
    Ok(())
}
