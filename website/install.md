---
title: Install
description: Documentation on installing dprint.
---

# Install Dprint

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
- [Homebrew](https://brew.sh/) (Mac):

      ```bash
      brew install dprint
      ```
- Cargo (builds and installs the [cargo package](https://crates.io/crates/dprint) from source):

      ```bash
      cargo install dprint
      ```

For binaries and source, see the [GitHub releases](https://github.com/dprint/dprint/releases).

## Editor Extensions

- [Visual Studio Code](https://marketplace.visualstudio.com/items?itemName=dprint.dprint)
- More to come!

Next step: [Setup](/setup)
