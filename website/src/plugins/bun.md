---
title: Bun Plugin
description: Documentation on the Bun code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Bun Plugin

Adapter plugin that formats files via the formatter of [Bun](https://bun.com) (`bun format`). `bun format` prints what [Prettier](https://prettier.io) prints, so this plugin is a way to get Prettier's formatting from a Wasm plugin.

It formats the following languages:

- JavaScript and TypeScript (including JSX and Flow)
- JSON, JSONC, and JSON5
- CSS, SCSS, and Less
- GraphQL
- YAML
- Markdown and MDX
- HTML, Vue, and MJML
- Handlebars
- TOML

Code embedded in these files is also formatted (ex. CSS in a JavaScript template literal, code blocks in Markdown, or the scripts and styles of an HTML or Vue file).

You do not need Bun installed because dprint runs Bun's formatter from the .wasm file in a sandboxed environment.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add bun
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"bun"` property to add configuration:

```json
{
  "bun": {
    // bun's config goes here
  },
  "plugins": [
    "npm:@dprint/bun@x.x.x"
  ]
}
```

## Configuration

See [Configuration](/plugins/bun/config)

The options are the ones that `bun format` reads in a configuration file, with the same names and values: [Prettier's options](https://prettier.io/docs/options), or with `"flavor": "oxfmt"` the ones of an `.oxfmtrc.json`.

Note that the plugin does not read a `.prettierrc`, an `.oxfmtrc.json`, an `.editorconfig`, a `tsconfig.json`, or a `bunfig.toml` because it runs sandboxed in a Wasm runtime and has no access to the file system.

### Disabling languages

Every language is formatted by default. To have the plugin leave the files of a language alone, set its `<language>.enabled` option to `false`:

```json
{
  "bun": {
    "markdown.enabled": false,
    "yaml.enabled": false
  }
}
```

The languages are `javascript`, `typescript`, `json`, `jsonc`, `json5`, `css`, `scss`, `less`, `yaml`, `markdown`, `mdx`, `graphql`, `handlebars`, `html`, `vue`, `mjml`, and `toml`. JSX is part of `javascript` and TSX is part of `typescript`. Prettier has no TOML, so it is formatted the way oxfmt formats it with every flavor.

A disabled language's files are no longer matched by this plugin, so dprint formats them with the next plugin that matches them, if any. This only decides which files are formatted: code embedded in a file of an enabled language is still formatted (ex. CSS in a JavaScript template literal when `css.enabled` is `false`).

### Sorting imports

As in `bun format`, the three plugins of Prettier that sort imports are built in. Name one in `plugins` to sort like it, with its options:

```json
{
  "bun": {
    "plugins": ["@ianvs/prettier-plugin-sort-imports"],
    "importOrder": [
      "<BUILTIN_MODULES>",
      "",
      "<THIRD_PARTY_MODULES>",
      "",
      "^@/(.*)$",
      "^[./]"
    ]
  }
}
```

- `@trivago/prettier-plugin-sort-imports` and `@ianvs/prettier-plugin-sort-imports`, with the `importOrder*` options.
- `prettier-plugin-organize-imports`, which also removes unused imports, with the `organizeImports*` options. It reads the JSX settings of a `tsconfig.json`, which the plugin cannot do, so set them as `tsconfig.jsx`, `tsconfig.jsxFactory`, `tsconfig.jsxFragmentFactory` and `tsconfig.reactNamespace` if a project does not use the defaults.

### The flavor of oxfmt

With `"flavor": "oxfmt"` the plugin prints what [oxfmt](https://oxc.rs/docs/guide/usage/formatter) prints where that is not what Prettier prints, and has the options of oxfmt: lines are 100 wide by default, `package.json` files are sorted (`sortPackageJson`), and `sortImports`, `jsdoc` and `insertFinalNewline` are available.

```json
{
  "bun": {
    "flavor": "oxfmt",
    "sortImports": true,
    "jsdoc": true
  }
}
```

### What is not supported

These need the file system or JavaScript packages, which a Wasm plugin has neither of:

- `overrides`. Use more than one dprint configuration file instead.
- `sortTailwindcss`, which asks the `tailwindcss` package of the project for the order of the classes.
- Plugins of Prettier other than the three above, and so the languages that only a plugin reads (ex. Svelte and Astro).

## Using with other plugins

This plugin formats many kinds of files, so it may match the same files as another plugin in your config file. The order of the `"plugins"` array defines the precedence, so list a plugin before this one to have it format the files they both match:

```json
{
  "plugins": [
    // formats .json files instead of the Bun plugin
    "npm:@dprint/json@x.x.x",
    "npm:@dprint/bun@x.x.x"
  ]
}
```

For more fine grained control, see [associations](/config/#associations).

## Playground

See [Playground](https://dprint.dev/playground#plugin/bun)

## Source

See https://github.com/dprint/dprint-plugin-bun
