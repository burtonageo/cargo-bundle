use Settings;
use std::error::Error;
use std::marker::{Send, Sync};
use std::path::PathBuf;

pub fn bundle_project(_settings: &Settings) -> Result<Vec<PathBuf>, Box<Error + Send + Sync>> {
    unimplemented!();
}
