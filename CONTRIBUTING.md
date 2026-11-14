# Contributing

Build, test and format with:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

New behavior needs a test that fails without it. The guide's
[contributing page](docs/src/contributing.md) describes the test setup and how
the documentation is kept in step with the code.
