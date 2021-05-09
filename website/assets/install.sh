#!/bin/sh
# Adapted from Deno's install script at https://github.com/denoland/deno_install/blob/main/install.ps1
# All rights reserved. MIT license.

set -e

case $(uname -s) in
Darwin) target="x86_64-apple-darwin" ;;
*) target="x86_64-unknown-linux-gnu" ;;
esac

if [ $(uname -m) != "x86_64" ]; then
	echo "Unsupported architecture $(uname -m). Only x64 binaries are available."
	exit 1
fi

if [ $# -eq 0 ]; then
	dprint_asset_path=$(
		command curl -sSf https://github.com/dprint/dprint/releases |
			command grep -o "/dprint/dprint/releases/download/.*/dprint-${target}\\.zip" |
			command head -n 1
	)
	if [ ! "$dprint_asset_path" ]; then exit 1; fi
	dprint_uri="https://github.com${dprint_asset_path}"
else
	dprint_uri="https://github.com/dprint/dprint/releases/download/${1}/dprint-${target}.zip"
fi

dprint_install="${DPRINT_INSTALL:-$HOME/.dprint}"
bin_dir="$dprint_install/bin"
exe="$bin_dir/dprint"

if [ ! -d "$bin_dir" ]; then
	mkdir -p "$bin_dir"
fi

# stop any running dprint editor services
pkill -9 "dprint" || true

# download and install
curl --fail --location --progress-bar --output "$exe.zip" "$dprint_uri"
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
