---
title: Incremental Formatting and Process Plugins - dprint 0.7
description: Overview of the new features in dprint 0.7
publish_date: 2020-08-05
author: David Sherret
---

dprint is a pluggable, configurable, and fast code formatting platform written in Rust.

This post will outline the new features in dprint 0.7.

## Incremental

When formatting a collection of files, there's not much point formatting a file that hasn't changed.

The CLI now has an "incremental" feature that only formats files that haven't changed since the last time they were formatted.

To use it, you may specify `"incremental": true` in your `.dprintrc.json` file (recommended):

```jsonc
{
  // etc...
  "incremental": true
  // etc...
}
```

This is specified by default when using `dprint init`.

Alternatively, use the `--incremental` flag on the CLI:

```bash
dprint fmt --incremental
```

Using this will drastically speed up performance. For example, in [Deno](https://github.com/denoland/deno)'s internal code, using it brought down formatting from ~2.4s to ~0.1s on the second run on my machine.

This incremental information is stored in dprint's cache folder. You may clear the cache by running `dprint clear-cache`.

## Process Plugins

dprint has previously only supported plugins compiled to a single `.wasm` file. Unfortunately, this has been limiting because not many languages support cleanly compiling to a `.wasm` file yet (for example, it's impossible to do this in C# at the time of this post).

To get around this, a new type of plugin called "process plugins" has been introduced. These are plugins that work by the CLI executing a separate process and then communicating with it via stdin and stdout.

Obviously, process plugins are not as secure as WASM plugins since they don't run sandboxed. To at least ensure the process plugin being used is the same that was built with the CI pipeline, you must specify the checksum of the file in addition to a URL. For example:

```jsonc
{
  // etc...
  "plugins": [
    "https://plugins.dprint.dev/roslyn-3.6.0.exe-plugin@2ef95b9c2ebfbd50a2bef422c8ad066563bf4d452da44386243e6d7915b0d62b"
  ]
}
```

Yeah, kind of annoying, but overall not too bad if you copy and paste. The instructions for setting this up are outlined on each plugin's page.

At the moment there are two process plugins (these should both work on Windows, Linux, and Mac):

- [dprint-plugin-roslyn](https://github.com/dprint/dprint-plugin-roslyn) - C# and Visual Basic formatting using the [Roslyn](https://github.com/dotnet/roslyn) compiler.
- [dprint-plugin-prettier](https://github.com/dprint/dprint-plugin-prettier) - Formats all the many languages [Prettier](https://prettier.io) supports. Note: if you wish to use this with plugins like `dprint-plugin-typescript`, then specify this after those plugins in the `"plugins"` array of the configuration file.

## Immediate Future

I've already started work on a [gofmt](https://golang.org/cmd/gofmt/) plugin. It will be released over the next few weeks when I find the time to finish it.

Thanks for reading!
