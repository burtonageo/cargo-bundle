# Cargo bundle

Wrap rust executables in OS-specific app bundles

## About

 `cargo-bundle` is a tool used to generate installers or app bundles for executables built with `cargo`.
It supports `.app` bundles for OSX, `.deb` and `.rpm` packages. `msi` installer support may be added soon.
In addition to bundling executables, it also has facilities to download and package extra data from other
sources such as the internet or other repositories.

 To start using `cargo bundle`, you should create a `Bundle.toml` file in the root directory of your project,
next to the `Cargo.toml` file. Your `Bundle.toml` describes various attributes of the generated bundle, such
as its name, icon, description, copyright, as well as any packaging scripts you need to generate extra data.
The full manifest format is described below. Note that by default, `cargo-bundle` will look at your `Cargo.toml`
for information such as your package version and description to avoid repeating yourself, but this can be
overridden.

## Bundle manifest format

TODO: write this.

## Contributing

 `cargo-bundle` has ambitions to be inclusive project and welcome contributions from anyone. However, this is
still a half thought-out project (see `Status` section). When I announce this project publicly, then things
will change.

## Status

Woefully incomplete. Do not use yet. The design is so incomplete that patches are not even useful at this
point.

## Dependencies

In addition to the Cargo dependencies, `cargo bundle` also requires `Cmake` and `OpenSSL`. Occasionally you may find that
building `OpenSSL` may cause a build error ; to fix this, see [these instructions](https://github.com/alexcrichton/ssh2-rs/issues/28).

## License

This program is licensed either under the terms of the [Apache Software License](http://www.apache.org/licenses/LICENSE-2.0.),
or the [MIT License](https://opensource.org/licenses/MIT). 
