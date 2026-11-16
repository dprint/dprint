---
title: Install
description: Documentation on installing dprint.
layout: layouts/documentation.njk
---

# Install dprint

Install using one of the methods below.

- Shell (Mac, Linux, FreeBSD, WSL, Termux):

  ```sh
  curl -fsSL https://dprint.dev/install.sh | sh
  ```

- Powershell (Windows):

  ```sh
  iwr https://dprint.dev/install.ps1 -useb | iex
  ```

- [npm](https://www.npmjs.com/):

  ```sh
  # globally
  npm install -g dprint

  # or for your project
  npm install dprint
  npx dprint help
  ```

- [Scoop](https://scoop.sh/) (Windows):

  ```sh
  scoop install dprint
  ```

- [Homebrew](https://brew.sh/) (Mac):

  ```sh
  brew install dprint
  ```

- [Cargo](https://crates.io/) (builds and installs the [cargo package](https://crates.io/crates/dprint) from source):

  ```sh
  # this will be slower since it builds from the source
  cargo install --locked dprint
  ```

- python/[uv](https://docs.astral.sh/uv/) via [https://github.com/trim21/dprint-py](https://github.com/trim21/dprint-py):

  ```sh
  uv add dprint-py
  uv run dprint help
  ```

- [mise](https://mise.jdx.dev):

  ```sh
  # globally
  mise use dprint --global

  # or for your project
  mise use dprint
  mise x dprint -- dprint help
  ```

- [packslip](https://packslip.dev):

  ```sh
  packslip install github.com/dprint/dprint
  ```

- [proto](https://moonrepo.dev/proto) (via its [community plugin](https://github.com/moonrepo/community-plugins/blob/master/tools/dprint.toml)):

  ```sh
  # globally
  proto install dprint --pin global

  # or for your project
  proto install dprint --pin
  ```

- [asdf-vm](https://asdf-vm.com/) ([asdf-dprint](https://github.com/asdf-community/asdf-dprint)):

  ```sh
  asdf plugin-add dprint https://github.com/asdf-community/asdf-dprint
  asdf install dprint latest
  ```

- [Arch Linux](https://aur.archlinux.org/packages/dprint):

  Install with any AUR helper, for example:

  ```sh
  paru -S dprint
  ```

  or binaries

  ```sh
  paru -S dprint-bin
  ```

For binaries and source, see the [GitHub releases](https://github.com/dprint/dprint/releases).

## Verify a Download

The release zips have [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations) that you can verify with the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify dprint-x86_64-unknown-linux-gnu.zip --repo dprint/dprint
```

Releases also include a signed [packslip](https://packslip.dev) (`packslip.sigstore.json`) describing the zips. Download it from the same release and run:

```sh
packslip verify packslip.sigstore.json --artifact dprint-x86_64-unknown-linux-gnu.zip
```

Each release also has a `SHASUMS256.txt` file and a table of SHA-256 checksums in its notes.

## Editor Extensions

- [Visual Studio Code](https://marketplace.visualstudio.com/items?itemName=dprint.dprint)
- [IntelliJ](https://plugins.jetbrains.com/plugin/18192-dprint) - Thanks to the developers at [Canva](https://canva.com)
- Neovim with [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig/blob/master/doc/configs.md#dprint)
- The `dprint lsp` subcommand provides code formatting over the language server protocol. This can be used to format in other editors such as Neovim, Helix, and Zed. See [Language Server](/lsp).

Next step: [Setup](/setup)
