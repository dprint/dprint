# dprint

[![CI](https://github.com/dprint/dprint/workflows/CI/badge.svg)](https://github.com/dprint/dprint/actions?query=workflow%3ACI)

Monorepo for dprint—a pluggable and configurable code formatting platform.

This project is under active early development. I recommend you check its output to ensure it's doing its job correctly and only run this on code that has been checked into source control.

## Links

- [Overview](https://dprint.dev/overview)
- [Getting Started](https://dprint.dev/install)
- [Playground](https://dprint.dev/playground)

## Plugins

- [dprint-plugin-typescript](https://github.com/dprint/dprint-plugin-typescript) - TypeScript/JavaScript code formatter.
- [dprint-plugin-json](https://github.com/dprint/dprint-plugin-json) - JSON/JSONC code formatter.
- [dprint-plugin-markdown](https://github.com/dprint/dprint-plugin-markdown) - Markdown code formatter.
- [dprint-plugin-rustfmt](https://github.com/dprint/dprint-plugin-rustfmt) - Rustfmt wrapper plugin.
- [dprint-plugin-roslyn](https://github.com/dprint/dprint-plugin-roslyn) - Roslyn (C#/VB) wrapper plugin.
- [dprint-plugin-prettier](https://github.com/dprint/dprint-plugin-prettier) - Prettier wrapper plugin.
- [dprint-plugin-yapf](https://github.com/dprint/dprint-plugin-yapf) - YAPF (Python) wrapper plugin.

## Notes

This repo is under active early development.

1. The interface between the CLI and plugins might change often. You may need to keep updating to the latest version of both the CLI and plugins (the CLI will let you know what to do).
   - An upgrade path will be outlined in the [release notes](https://github.com/dprint/dprint/releases) when this occurs.
2. I do a lot of this development in my spare time. Please consider [sponsoring](https://dprint.dev/sponsor) if you are a commercial company using this.
