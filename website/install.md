---
title: Install
description: Documentation on installing dprint.
---

# Install dprint

Install using one of the methods below.

- Shell (Mac, Linux, WSL):

      ```bash
      curl -fsSL https://dprint.dev/install.sh | sh
      ```
- Windows Installer

  [Download](https://github.com/dprint/dprint/releases/latest/download/dprint-x86_64-pc-windows-msvc-installer.exe)
- Powershell (Windows):

      ```powershell
      iwr https://dprint.dev/install.ps1 -useb | iex
      ```

- [Scoop](https://scoop.sh/) (Windows):

      ```powershell
      scoop install dprint
      ```

- [Homebrew](https://brew.sh/) (Mac):

      ```bash
      brew install dprint
      ```
- [Cargo](https://crates.io/) (builds and installs the [cargo package](https://crates.io/crates/dprint) from source):

      ```bash
      cargo install dprint
      ```

- [npm](https://www.npmjs.com/):

      ```bash
      npm install dprint
      npx dprint help

      # or install globally
      npm install -g dprint
      dprint help
      ```

- [asdf-vm](https://asdf-vm.com/) ([asdf-dprint](https://github.com/asdf-community/asdf-dprint)):

      ```bash
      asdf plugin-add dprint https://github.com/asdf-community/asdf-dprint
      asdf install dprint latest
      ```

- [bvm](https://github.com/bvm/bvm) (Experimental)

      ```bash
      bvm registry add https://bvm.land/dprint/registry.json
      bvm install dprint
      ```

For binaries and source, see the [GitHub releases](https://github.com/dprint/dprint/releases).

## Editor Extensions

- [Visual Studio Code](https://marketplace.visualstudio.com/items?itemName=dprint.dprint)
- [IntelliJ](https://plugins.jetbrains.com/plugin/18192-dprint) - Thanks to the developers at [Canva](https://canva.com)
- More to come!

Next step: [Setup](/setup)
