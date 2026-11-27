---
title: tidy Plugin
description: Documentation on the tidy code formatting plugin for dprint.
layout: layouts/plugin.njk
---

Wrapper plugin that formats files via [tidy](https://github.com/lukeed/tidy), a formatter that prints what [Prettier](https://prettier.io) prints.

It formats the following languages:

- HTML and MJML
- Vue
- Handlebars
- CSS, SCSS, and Less
- JSON, JSONC, and JSON5
- YAML
- TOML
- XML (including SVG)
- GraphQL
- INI (including gitconfig)
- Dockerfiles
- Markdown
- JavaScript and TypeScript (including JSX)

Code embedded in these files is also formatted when tidy knows its language (ex. the scripts and styles of an HTML or Vue file, or code blocks in Markdown).

You do not need tidy installed because dprint runs it from the .wasm file in a sandboxed environment.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add tidy
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"tidy"` property to add configuration:

```json
{
  "tidy": {
    // tidy's config goes here
  },
  "plugins": [
    "npm:@dprint/tidy@x.x.x"
  ]
}
```

## Configuration

The options are the ones of a [`tidy.json`](https://tidy.lukeed.com/docs/options/), with the same names and values. The options of a language are written with the language's section first (ex. `"js.semi": false`), or in an object named after the section as in a `tidy.json` (ex. `"js": { "semi": false }`). An option every language has, like `width` or `quote`, can also be set for a single section:

```json
{
  "tidy": {
    "quote": "single",
    "js.semi": false,
    "markdown.width": 120,
    "markdown.wrap": "always"
  }
}
```

The global `lineWidth`, `indentWidth`, `useTabs`, and `newLineKind` are used as tidy's `width`, `indent`, `tabs`, and `eol` when they're set.

Note that the plugin does not read a `tidy.json` or an `.editorconfig` because it runs sandboxed in a Wasm runtime and has no access to the file system.

### Disabling languages

Every language is formatted by default. To have the plugin leave the files of a language alone, set its `<section>.enabled` option to `false`:

```json
{
  "tidy": {
    "markdown.enabled": false,
    "toml.enabled": false
  }
}
```

The sections are `html`, `vue`, `handlebars`, `css`, `json`, `yaml`, `toml`, `xml`, `markdown`, `dockerfile`, `ini`, `graphql`, and `js`. MJML is part of `html`, SCSS and Less are part of `css`, and TypeScript is part of `js`.

A disabled language's files are no longer matched by this plugin, so dprint formats them with the next plugin that matches them, if any. This only decides which files are formatted: code embedded in a file of an enabled language is still formatted (ex. the `<script>` of an HTML file when `js.enabled` is `false`).

## Using with other plugins

This plugin formats many kinds of files, so it may match the same files as another plugin in your config file. The order of the `"plugins"` array defines the precedence, so list a plugin before this one to have it format the files they both match:

```json
{
  "plugins": [
    // formats .json files instead of the tidy plugin
    "npm:@dprint/json@x.x.x",
    "npm:@dprint/tidy@x.x.x"
  ]
}
```

For more fine grained control, see [associations](/config/#associations) or see [disabling languages](#disabling-languages).
