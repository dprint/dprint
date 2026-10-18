#!/bin/bash
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
# the plugin prints to stdout/stderr via wasi, whose imports newer versions of
# rust-lld won't leave undefined without this
export RUSTFLAGS="-C link-arg=--allow-undefined"
cargo build --release --target=wasm32-unknown-unknown && cp ./target/wasm32-unknown-unknown/release/test_plugin.wasm test_plugin.wasm
