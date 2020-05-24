cargo build --target=wasm32-unknown-unknown --release
Move-Item -Path target/wasm32-unknown-unknown/release/test_plugin.wasm -Destination ../test_plugin.wasm -Force