# Cargo bundle

__Wrap rust executables in OS-specific app bundles__

## About

 `cargo-bundle` is a tool used to generate installers or app bundles for executables built with `cargo`.
It supports `.app` bundles for OSX, `.deb` and `.rpm` packages. `msi` installer support may be added soon.
In addition to bundling executables, it also has facilities to download and package extra data from other
sources such as the internet or other repositories.

## Status

Woefully incomplete. Do not use yet. The design is so incomplete that patches are not even useful at this
point.

## License

This program is licensed either under the terms of the [Apache Software License](http://www.apache.org/licenses/LICENSE-2.0.),
or the [MIT License](https://opensource.org/licenses/MIT). 
