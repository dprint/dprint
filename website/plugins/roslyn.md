---
title: Roslyn Plugin (C#/VB)
description: Documentation on the Roslyn code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/roslyn">Roslyn</a></li>
  </ul>
</nav>

# Roslyn Plugin

Wrapper plugin that formats C# and Visual Basic code via [Roslyn](https://github.com/dotnet/roslyn).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike WASM plugins).
  </div>
</div>

## Install and Setup

In _.dprintrc.json_:

1. Specify the plugin url in the `"plugins"` array (follow instructions at [https://github.com/dprint/dprint-plugin-roslyn/releases/](https://github.com/dprint/dprint-plugin-roslyn/releases/)).
2. Ensure `.cs` and `.vb` file extensions are matched in an `"includes"` pattern.
3. Add a `"roslyn"` configuration property if desired.

```jsonc
{
  // ...etc...
  "roslyn": {
    "csharp.indentBlock": false,
    "visualBasic.indentWidth": 2
  }
}
```

## Configuration

C# configuration uses the [`CSharpFormattingOptions`](https://docs.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.formatting.csharpformattingoptions?view=roslyn-dotnet) (use `"csharp.<property name goes here>": <value goes here>` in the configuration file).

It does not seem like Roslyn supports any VB specific configuration.
