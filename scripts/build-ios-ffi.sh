#!/bin/sh
# Build the oratio-core FFI (whisper included) as an XCFramework for iOS
# and regenerate the Swift bindings. Run after changing crates/oratio-ffi
# or crates/oratio-core.
set -e
cd "$(dirname "$0")/.."

rustup target add aarch64-apple-ios aarch64-apple-ios-sim >/dev/null

cargo build -p oratio-ffi --release --target aarch64-apple-ios
cargo build -p oratio-ffi --release --target aarch64-apple-ios-sim
cargo build -p oratio-ffi --release

cargo run -p oratio-ffi --release --bin uniffi-bindgen -- generate \
    --library target/release/liboratio_ffi.a --language swift --out-dir ios/Generated

HDRS=target/ios-ffi-headers
rm -rf "$HDRS" ios/OratioFFI.xcframework
mkdir -p "$HDRS"
cp ios/Generated/oratio_ffiFFI.h "$HDRS/"
cp ios/Generated/oratio_ffiFFI.modulemap "$HDRS/module.modulemap"

xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/liboratio_ffi.a -headers "$HDRS" \
    -library target/aarch64-apple-ios-sim/release/liboratio_ffi.a -headers "$HDRS" \
    -output ios/OratioFFI.xcframework

echo "OK: ios/OratioFFI.xcframework + ios/Generated/oratio_ffi.swift"
