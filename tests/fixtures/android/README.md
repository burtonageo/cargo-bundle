These packaging fixtures come from rust-apk 1.0.0 (MIT OR Apache-2.0),
https://tangled.org/philocalyst.bsky.social/Apk-rs/commit/4b01345bd055f04b391ad4eea5634f7ad794df88.

`libpostcard.so` is an aarch64-linux-android24 shared library compiled from
`activity.c`; it exports a no-op NativeActivity entry point. Link using
`rust-lld -flavor gnu -shared -m aarch64linux -z max-page-size=16384 -soname libpostcard.so`.

`test-only.pem` is a public test P-256 signing key and certificate. Never use
this key for distributing applications.
