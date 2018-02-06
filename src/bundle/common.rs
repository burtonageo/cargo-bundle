use ::Error;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use term;
use walkdir::WalkDir;

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
    let parent = match path.parent() {
        Some(dir) => dir,
        None => bail!("Path has no parent: {:?}", path),
    };
    fs::create_dir_all(parent)?;
    let file = File::create(path)?;
    Ok(BufWriter::new(file))
}

/// Copies the file or directory (recursively) at `from` into the directory at
/// `to`.  The `to` directory (and its parents) will be created if necessary.
pub fn copy_to_dir(from: &Path, to_dir: &Path) -> ::Result<()> {
    let parent = match from.parent() {
        Some(dir) => dir,
        None => bail!("Path has no parent: {:?}", from),
    };
    for entry in WalkDir::new(from) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel_path = entry.path().strip_prefix(parent).unwrap();
            let dest_path = to_dir.join(rel_path);
            let dest_dir = dest_path.parent().unwrap();
            fs::create_dir_all(dest_dir)?;
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
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
    let mut output = match term::stdout() {
        Some(terminal) => terminal,
        None => bail!("Can't write to stdout"),
    };
    output.attr(term::Attr::Bold)?;
    output.fg(term::color::GREEN)?;
    write!(output, "    {}", step)?;
    output.reset()?;
    write!(output, " {}\n", msg)?;
    output.flush()?;
    Ok(())
}

/// Prints a warning message to stdout, in the same format that `cargo` uses.
pub fn print_warning(message: &str) -> ::Result<()> {
    let mut output = match term::stdout() {
        Some(terminal) => terminal,
        None => bail!("Can't write to stdout"),
    };
    output.attr(term::Attr::Bold)?;
    output.fg(term::color::YELLOW)?;
    write!(output, "warning:")?;
    output.reset()?;
    write!(output, " {}\n", message)?;
    output.flush()?;
    Ok(())
}

/// Prints an error to stdout, in the same format that `cargo` uses.
pub fn print_error(error: &Error) -> ::Result<()> {
    let mut output = match term::stdout() {
        Some(terminal) => terminal,
        None => bail!("Can't write to stdout"),
    };
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
}
