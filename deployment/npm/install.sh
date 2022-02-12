#!/bin/sh
set -e

if ! command -v unzip >/dev/null; then
	echo "Error: unzip is required to install dprint." 1>&2
	exit 1
fi

case $(uname -sm) in
"Darwin x86_64") target="x86_64-apple-darwin" ;;
"Darwin arm64") target="aarch64-apple-darwin" ;;
*) target="x86_64-unknown-linux-gnu" ;;
esac

dprint_uri="https://github.com/dprint/dprint/releases/download/${1}/dprint-${target}.zip"
exe="dprint"

# download
curl --fail --location --progress-bar --output "$exe.zip" "$dprint_uri"

# verify zip checksum
node install_verify_checksum.js

# unzip
unzip -o "$exe.zip"
chmod +x "$exe"
rm "$exe.zip"
