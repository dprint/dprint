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
		"Linux aarch64") target="aarch64-unknown-linux" ;;
		"Linux loongarch64") target="loongarch64-unknown-linux" ;;
		"Linux riscv64") target="riscv64gc-unknown-linux-gnu" ;; # riscv64 build only has a GNU libc variant.
		"Linux ppc64le") target="powerpc64le-unknown-linux-gnu" ;; # ppc64le build only has a GNU libc variant.
		*) target="x86_64-unknown-linux" ;;
	esac
fi
if [ "${target%-linux}" != "$target" ]; then # check "-linux" suffix
	is_musl=$(ldd /bin/sh | grep 'musl' || true)
	if [ -z "$is_musl" ]; then
		target="$target-gnu"
	else
		target="$target-musl"
	fi
fi

if [ $# -eq 0 ]; then
	dprint_uri="https://github.com/dprint/dprint/releases/latest/download/dprint-${target}.zip"
else
	dprint_uri="https://github.com/dprint/dprint/releases/download/${1}/dprint-${target}.zip"
fi

dprint_install="${DPRINT_INSTALL:-$HOME/.dprint}"
bin_dir="$dprint_install/bin"
if [ ! -d "$bin_dir" ]; then
	mkdir -p "$bin_dir"
fi
dprint_install="$(realpath "$dprint_install")"
bin_dir="$dprint_install/bin"

exe="$bin_dir/dprint"
zip="$exe.zip"

# append .exe for Windows
if [ "$target" = "x86_64-pc-windows-msvc" ]; then
	exe="$exe.exe"
fi

# download
curl --fail --location --progress-bar --output "$zip" "$dprint_uri"

# stop any running dprint editor services
pkill -9 "dprint" || true

# install
cd "$bin_dir"
unzip -o "$zip"
chmod +x "$exe"
rm "$zip"

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
