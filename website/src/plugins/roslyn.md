---
title: Roslyn Plugin (C#/VB)
description: Documentation on the Roslyn code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/roslyn">Roslyn</a></li>
  </ul>
</nav>

# Roslyn Plugin

Adapter plugin that formats C# and Visual Basic code via [Roslyn](https://github.com/dotnet/roslyn).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add roslyn
# or install from npm
dprint add npm:@dprint/roslyn
```

This will update your config file to have an entry for the plugin. Then optionally specify a `"roslyn"` property to add configuration:

```json
{
  "roslyn": {
    // roslyn's config goes here
  }
  // etc...
}
```

## Configuration

C# configuration uses the [`CSharpFormattingOptions`](https://docs.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.formatting.csharpformattingoptions?view=roslyn-dotnet) (use `"csharp.<property name goes here>": <value goes here>` in the configuration file).

It does not seem like Roslyn supports any VB specific configuration.
