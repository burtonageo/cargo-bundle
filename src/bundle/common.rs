use ::ResultExt;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use term;

/// Returns true if the path has a filename indicating that it is a high-desity
/// "retina" icon.  Specifically, returns true the the file stem ends with
/// "@2x" (a convention specified by the [Apple developer docs](
/// https://developer.apple.com/library/mac/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Optimizing/Optimizing.html)).
pub fn is_retina<P: AsRef<Path>>(path: P) -> bool {
    path.as_ref()
        .file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.ends_with("@2x"))
        .unwrap_or(false)
}

/// Creates a new file at the given path, creating any parent directories as
/// needed.
pub fn create_file(path: &Path) -> ::Result<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(&parent).chain_err(|| {
            format!("Failed to create directory {:?}", parent)
        })?;
    }
    let file = File::create(path).chain_err(|| {
        format!("Failed to create file {:?}", path)
    })?;
    Ok(BufWriter::new(file))
}

/// Copies a regular file from one path to another, creating any parent
/// directories of the destination path as necessary.  Fails if the source path
/// is a directory or doesn't exist.
pub fn copy_file(from: &Path, to: &Path) -> ::Result<()> {
    if !from.exists() {
        bail!("{:?} does not exist", from);
    }
    if !from.is_file() {
        bail!("{:?} is not a file", from);
    }
    let dest_dir = to.parent().unwrap();
    fs::create_dir_all(dest_dir).chain_err(|| {
        format!("Failed to create {:?}", dest_dir)
    })?;
    fs::copy(from, to).chain_err(|| {
        format!("Failed to copy {:?} to {:?}", from, to)
    })?;
    Ok(())
}

/// Given a path (absolute or relative) to a resource file, returns the
/// relative path from the bundle resources directory where that resource
/// should be stored.
pub fn resource_relpath(path: &Path) -> PathBuf {
    let mut dest = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {}
            Component::RootDir => dest.push("_root_"),
            Component::CurDir => {}
            Component::ParentDir => dest.push("_up_"),
            Component::Normal(string) => dest.push(string),
        }
    }
    dest
}

/// Prints a message to stdout, in the same format that `cargo` uses,
/// indicating that we are creating a bundle with the given filename.
pub fn print_bundling(filename: &str) -> ::Result<()> {
    print_progress("Bundling", filename)
}

/// Prints a message to stdout, in the same format that `cargo` uses,
/// indicating that we have finished the the given bundles.
pub fn print_finished(output_paths: &Vec<PathBuf>) -> ::Result<()> {
    let pluralised = if output_paths.len() == 1 {
        "bundle"
    } else {
        "bundles"
    };
    let msg = format!("{} {} at:", output_paths.len(), pluralised);
    print_progress("Finished", &msg)?;
    for path in output_paths {
        println!("        {}", path.display());
    }
    Ok(())
}

fn print_progress(step: &str, msg: &str) -> ::Result<()> {
    if let Some(mut output) = term::stdout() {
        output.attr(term::Attr::Bold)?;
        output.fg(term::color::GREEN)?;
        write!(output, "    {}", step)?;
        output.reset()?;
        write!(output, " {}\n", msg)?;
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stdout();
        write!(output, "    {}", step)?;
        write!(output, " {}\n", msg)?;
        output.flush()?;
        Ok(())
    }
}

/// Prints a warning message to stdout, in the same format that `cargo` uses.
pub fn print_warning(message: &str) -> ::Result<()> {
    if let Some(mut output) = term::stdout() {
        output.attr(term::Attr::Bold)?;
        output.fg(term::color::YELLOW)?;
        write!(output, "warning:")?;
        output.reset()?;
        write!(output, " {}\n", message)?;
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stdout();
        write!(output, "warning:")?;
        write!(output, " {}\n", message)?;
        output.flush()?;
        Ok(())
    }
}

/// Prints an error to stdout, in the same format that `cargo` uses.
pub fn print_error(error: &::Error) -> ::Result<()> {
    if let Some(mut output) = term::stdout() {
        output.attr(term::Attr::Bold)?;
        output.fg(term::color::RED)?;
        write!(output, "error:")?;
        output.reset()?;
        output.attr(term::Attr::Bold)?;
        writeln!(output, " {}", error)?;
        output.reset()?;
        for cause in error.iter().skip(1) {
            writeln!(output, "  Caused by: {}", cause)?;
        }
        if let Some(backtrace) = error.backtrace() {
            writeln!(output, "{:?}", backtrace)?;
        }
        output.flush()?;
        Ok(())
    } else {
        let mut output = io::stdout();
        write!(output, "error:")?;
        writeln!(output, " {}", error)?;
        for cause in error.iter().skip(1) {
            writeln!(output, "  Caused by: {}", cause)?;
        }
        if let Some(backtrace) = error.backtrace() {
            writeln!(output, "{:?}", backtrace)?;
        }
        output.flush()?;
        Ok(())
    }
}
