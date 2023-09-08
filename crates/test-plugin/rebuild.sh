#!/bin/bash
cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
cargo build --release --target=wasm32-unknown-unknown && cp ./target/wasm32-unknown-unknown/release/test_plugin.wasm test_plugin.wasm
