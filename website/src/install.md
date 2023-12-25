---
title: Install
description: Documentation on installing dprint.
layout: layouts/documentation.njk
---

# Install dprint

Install using one of the methods below.

- Shell (Mac, Linux, WSL):

  ```sh
  curl -fsSL https://dprint.dev/install.sh | sh
  ```

- Windows Installer

  [Download](https://github.com/dprint/dprint/releases/latest/download/dprint-x86_64-pc-windows-msvc-installer.exe)
- Powershell (Windows):

  ```sh
  iwr https://dprint.dev/install.ps1 -useb | iex
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

- [Deno](https://deno.land):

  For just your project, add a [deno task](https://deno.land/manual/tools/task_runner) to your deno.json file:

  ```json
  {
    "tasks": {
      "fmt": "deno task dprint fmt",
      "fmt:check": "deno task dprint check",
      "dprint": "deno run -A npm:dprint"
    }
  }
  ```

  Then run `deno task dprint init` to initialize and format by running: `deno task fmt`

  Also, you could install it globally via Deno, but like npm it has a startup and memory cost since it needs to run Deno then run dprint. It's recommended to install it globally via another method.

  ```sh
  deno install -A npm:dprint
  dprint help
  ```

- [npm](https://www.npmjs.com/):

  ```sh
  # for your project
  npm install dprint
  npx dprint help

  # or install globally (not recommended because it has a startup and memory cost)
  npm install -g dprint
  dprint help
  ```

- [asdf-vm](https://asdf-vm.com/) ([asdf-dprint](https://github.com/asdf-community/asdf-dprint)):

  ```sh
  asdf plugin-add dprint https://github.com/asdf-community/asdf-dprint
  asdf install dprint latest
  ```

For binaries and source, see the [GitHub releases](https://github.com/dprint/dprint/releases).

## Editor Extensions

- [Visual Studio Code](https://marketplace.visualstudio.com/items?itemName=dprint.dprint)
- [IntelliJ](https://plugins.jetbrains.com/plugin/18192-dprint) - Thanks to the developers at [Canva](https://canva.com)
- The `dprint lsp` subcommand (requires dprint 0.45+) provides code formatting over the language server protocol. This can be used to format in other editors.

Next step: [Setup](/setup)
