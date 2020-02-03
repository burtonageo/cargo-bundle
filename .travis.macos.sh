#!/bin/sh
set -e
set -x
export CARGO_BUNDLE="`pwd`/target/debug/cargo-bundle"
export RUST_BACKTRACE=1

cargo build --verbose

rustup target add x86_64-apple-ios;
RUNTIME_ID=$(xcrun simctl list runtimes | grep iOS | cut -d ' ' -f 7 | tail -1)
export SIM_ID=$(xcrun simctl create My-iphone7 com.apple.CoreSimulator.SimDeviceType.iPhone-7 $RUNTIME_ID)
xcrun simctl boot $SIM_ID
$CARGO_BUNDLE bundle --example hello --target x86_64-apple-ios
xcrun simctl install $SIM_ID target/x86_64-apple-ios/debug/examples/bundle/ios/hello.app
xcrun simctl launch $SIM_ID io.github.burtonageo.cargo-bundle.hello
