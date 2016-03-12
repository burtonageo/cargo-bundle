use Settings;
use std::error::Error;
use std::marker::{Send, Sync};

pub fn bundle_project(_settings: &Settings) -> Result<(), Box<Error + Send + Sync>> {
    unimplemented!();
}
