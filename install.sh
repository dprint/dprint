#!/bin/sh
# Adapted from Deno's install script at https://github.com/denoland/deno_install/blob/main/install.sh
# All rights reserved. MIT license.

set -e

if ! command -v unzip >/dev/null; then
	echo "Error: unzip is required to install dprint." 1>&2
	exit 1
fi

if [ "$OS" = "Windows_NT" ]; then
	target="x86_64-pc-windows-msvc"
else
	case $(uname -sm) in
	"Darwin x86_64") target="x86_64-apple-darwin" ;;
	"Darwin arm64") target="aarch64-apple-darwin" ;;
	"Linux aarch64") target="aarch64-unknown-linux-gnu" ;;
	*) target="x86_64-unknown-linux-gnu" ;;
	esac
fi

if [ $# -eq 0 ]; then
	dprint_uri="https://github.com/dprint/dprint/releases/latest/download/dprint-${target}.zip"
else
	dprint_uri="https://github.com/dprint/dprint/releases/download/${1}/dprint-${target}.zip"
fi

dprint_install="${DPRINT_INSTALL:-$HOME/.dprint}"
bin_dir="$dprint_install/bin"
exe="$bin_dir/dprint"

if [ ! -d "$bin_dir" ]; then
	mkdir -p "$bin_dir"
fi

# download
curl --fail --location --progress-bar --output "$exe.zip" "$dprint_uri"

# stop any running dprint editor services
pkill -9 "dprint" || true

# install
cd "$bin_dir"
unzip -o "$exe.zip"
chmod +x "$exe"
rm "$exe.zip"

echo "dprint was installed successfully to $exe"
if command -v dprint >/dev/null; then
	echo "Run 'dprint --help' to get started"
else
	case $SHELL in
	/bin/zsh) shell_profile=".zshrc" ;;
	*) shell_profile=".bash_profile" ;;
	esac
	echo "Manually add the directory to your \$HOME/$shell_profile (or similar)"
	echo "  export DPRINT_INSTALL=\"$dprint_install\""
	echo "  export PATH=\"\$DPRINT_INSTALL/bin:\$PATH\""
	echo "Run '$exe --help' to get started"
fi
