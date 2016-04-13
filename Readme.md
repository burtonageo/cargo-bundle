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

 To build a bundle, simply run `cargo bundle` in your project's directory (where the `Bundle.toml` is placed).
If you would like to bundle a release build, you must add the `--release` flag to your call.

## Flags

 TODO(burtonageo): Write this

## Bundle manifest format

 There are several fields in the `Bundle.toml` file.

 * `name`: The name of the built application. If this is not present, then it will use the `name` value from
           your `Cargo.toml` file.
 * `identifier`: [REQUIRED] Unique identifier for your application. This is a simple string, but it may change so that
                 you can specify it for individual platforms.
 * `icon` : [REQUIRED] The icon used for your application (Unimplemented). TODO(burtonageo): image formats?
 * `version`: [OPTIONAL] The version of the application. If this is not present, then it will use the `version`
              value from your `Cargo.toml` file.
 * `resources`: [OPTIONAL] List of files or directories which will be copied to the resources section of the
                bundle. This section must be present, but it can be empty.
 * `script`: [OPTIONAL] This is a reserved field; at the moment it is not used for anything, but may be used to
             run scripts while packaging the bundle (e.g. download files, compress and encrypt, etc.).
 * `copyright`: [OPTIONAL] This contains a copyright string associated with your application.

### Example `Bundle.toml`:

```toml

name = "ExampleApplication"
identifier = "com.doe.exampleapplication"
version = "1.0.0"
resources = ["assets", "configuration", "secrets/public_key.txt"]
copyright = "Copyright (c) Jane Doe 2016. All rights reserved."

```

## Contributing

 `cargo-bundle` has ambitions to be inclusive project and welcome contributions from anyone. Please abide
by the rust code of conduct.

## Status

 Very early alpha. Expect the format of the `Bundle.toml` file to change, and there is no guarantee
of stability.

## License

This program is licensed either under the terms of the [Apache Software License](http://www.apache.org/licenses/LICENSE-2.0.),
or the [MIT License](https://opensource.org/licenses/MIT).

