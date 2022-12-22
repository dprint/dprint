#!/bin/bash
cargo build --release --target=wasm32-unknown-unknown && cp ./target/wasm32-unknown-unknown/release/test_plugin.wasm test_plugin.wasm
