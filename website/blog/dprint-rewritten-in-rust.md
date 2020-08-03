---
title: Dprint Rewritten in Rust
description: Overview of the new rust-based pluggable, configurable, and fast code formatting platform.
publish_date: 2020-07-15
author: David Sherret
---

Dprint is a pluggable, configurable, and fast code formatting platform now written in Rust.

I've been working on dprint since June 2019. It started as a Node project creating a TypeScript code formatter that worked the way I wanted in my personal open source projects. It's progressed a long way and I'll walk through some of the highlights in this post.

## Short Video Demonstrations

Editor demo with "format on save" enabled:

<video width="600" height="366" poster="/videos/editor-demo-thumbnail.png" controls>
    <source src="/videos/editor-demo.mp4" type="video/mp4">
    <p>Your browser doesn't support HTML5 video. See video <a href="/videos/editor-demo.mp4">here</a>.
</video>

CLI demo:

<video width="600" height="366" poster="/videos/cli-demo-thumbnail.png" controls>
    <source src="/videos/cli-demo.mp4" type="video/mp4">
    <p>Your browser doesn't support HTML5 video. See video <a href="/videos/cli-demo.mp4">here</a>.
</video>

## Single Executable

Dprint is distributed as a single executable with no dependencies. You may build it from the source or install it using one of the methods outlined on the [install page](/install).

## WebAssembly Formatter Plugins

The dprint CLI (command line interface/executable) has no knowledge of how to format code in a specific language. This is left up to the plugins. For example, if you want to format JSON code then you must specify to use a JSON code formatting plugin.

Plugins are specified per codebase in a configuration file (_.dprintrc.json_) found at the root directory or `config` folder of a codebase.

```json
{
  // ...omitted...
  "plugins": [
    // these may be urls or file paths
    "https://plugins.dprint.dev/typescript-x.x.x.wasm", // supports TypeScript and JavaScript
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/rustfmt-x.x.x.wasm",
    "https://plugins.dprint.dev/markdown-x.x.x.wasm"
  ]
}
```

As you can see above, plugins are distributed as WebAssembly files. This means they're portable across systems and run sandboxed when executed. Plugins have no network or file system access—they only receive text to format and provide a text result.

On first run, dprint will take a few seconds to download, compile, and cache the specified plugins in parallel. After that, they load in about 5-40ms on my machine depending on the plugin size.

## Language Support (Plugins)

- [Typescript / JavaScript](/plugins/typescript)
- [JSON](/plugins/json)
- [Markdown](/plugins/markdown)
- [Rust](/plugins/rustfmt) via Rustfmt

More languages will be added over time.

## Performance

Dprint is the fastest code formatter for TypeScript, JSON, and Markdown code that I know of.

For example, [Deno](https://deno.land/) recently switched from prettier to dprint for their internal code formatting and TypeScript, JSON, and Markdown formatting time dropped from **14.7s** to **2.2s** on my machine.

## Using Plugins Outside the CLI

Since dprint plugins are distributed as WebAssembly files, they have the added benefit of being usable in other environments such as the browser.

For example, this code uses the Rustfmt dprint plugin to format Rust code in Deno:

```ts
// documentation: https://doc.deno.land/https/dprint.dev/formatter/v2.ts
import { createStreaming } from "https://dprint.dev/formatter/v2.ts";

const globalConfig = {
    indentWidth: 2,
    lineWidth: 80,
};
const rustFormatter = await createStreaming(
    fetch("https://plugins.dprint.dev/rustfmt-x.x.x.wasm"),
);

rustFormatter.setConfig(globalConfig, { brace_style: "AlwaysNextLine" });

// outputs "fn test()\n{\n  println!("test")\n}\n"
console.log(rustFormatter.formatText("file.rs", "fn test() {println!(\"test\")}"));
```

## Configuration

The amount of configuration a plugin offers is up to the plugin itself. A plugin may have zero configuration or a lot. The existing plugins are highly configurable to allow you to format code closer to your preferences rather than my prescriptions.

Here's an example of how a `.dprintrc.json` file might look like:

```json
{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "openSource",
  "lineWidth": 160,
  "typescript": {
    "arrowFunction.useParentheses": "preferNone",
    "bracePosition": "nextLine",
    "preferHanging": true,
    "semiColons": "asi",
    "singleBodyPosition": "nextLine"
  },
  "json": {
    "indentWidth": 2
  },
  "includes": [
    "**/*.{ts,tsx,js,jsx,json,md,rs}"
  ],
  "excludes": [
    "**/node_modules",
    "**/dist",
    "**/target",
    "**/*-lock.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm",
    "https://plugins.dprint.dev/rustfmt-x.x.x.wasm",
    "https://plugins.dprint.dev/markdown-x.x.x.wasm"
  ]
}
```

To help create a `.dprintrc.json` file in your codebase, you may consider running the `dprint init` command.

## Extending Configurations

You may extend other configuration files by specifying an `extends` property. This may be a file path, URL, or relative path (remote configuration may extend other configuration files via a relative path).

<!-- dprint-ignore -->

```json
{
  "extends": "https://dprint.dev/path/to/config/file.v1.json",
  // ...omitted...
}
```

Referencing multiple configuration files is also supported. These should be ordered by precedence:

```json
{
  "extends": [
    "https://dprint.dev/path/to/config/file.v1.json",
    "https://dprint.dev/path/to/config/other.v1.json"
  ]
}
```

## Opinionated Configuration

The decision to use an opinionated configuration is one you can make within dprint itself. Dprint provides a way to distribute "locked" configurations.

This can be done by specifying a `"locked": true` property on the plugin's configuration.

For example, say the following configuration file was hosted at `https://dprint.dev/configs/my-config.json`:

```json
{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "typescript": {
    "locked": true, // note this property
    "lineWidth": 80,
    "indentWidth": 2,
    "useTabs": false,
    "quoteStyle": "preferSingle",
    "binaryExpression.operatorPosition": "sameLine"
  },
  "json": {
    "locked": true, // note this property
    "lineWidth": 80,
    "indentWidth": 2,
    "useTabs": false
  }
}
```

This would allow people to use it like so:

```json
{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "extends": "https://dprint.dev/configs/my-config.json",
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

But consumers specifying properties in the `"typescript"` or `"json"` objects of their config file would cause an error when running in the CLI:

```json
{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "extends": "https://dprint.dev/configs/my-config.json",
  "typescript": {
    "useBraces": "always" // error, "typescript" config was locked
  },
  "json": {
    "lineWidth": 120 // error, "json" config was locked
  },
  "plugins": [
    "https://plugins.dprint.dev/typescript-x.x.x.wasm",
    "https://plugins.dprint.dev/json-x.x.x.wasm"
  ]
}
```

For more information on configuration files beyond what's outlined here, see the [configuration documentation](/config).

## Opinionated White-Labeled Binaries

If you wish to have an opinionated white-labeled binaries locked to a specific configuration for your company, please [get in touch](/contact) as I will offer this as a separate service.

## Sponsorship Requirement for Commercial Maintainers

Dprint is and will always be free for formatting open source projects whose primary maintainer is not a for-profit company. Unfortunately dprint's growth isn't sustainable without support from for-profit companies and this support would help drive this project forward. If you wish to use dprint on a code base whose primary maintainer is a for-profit company or individual, then you must sponsor the project. See [https://dprint.dev/sponsor](https://dprint.dev/sponsor) for more details.

## Future

Dprint is still in the early stages, so there's still a lot of work to do. Overall, the main goals are to...

1. Continue improving support for existing languages.
2. Expand support to other languages.
3. Develop debugging and analysis tools to make dprint faster and speed up development.
4. Improve the underlying core algorithm to support more scenarios.

Thanks for reading!
