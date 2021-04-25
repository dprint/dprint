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
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

Follow the instructions at [https://github.com/dprint/dprint-plugin-roslyn/releases/](https://github.com/dprint/dprint-plugin-roslyn/releases/)

## Configuration

C# configuration uses the [`CSharpFormattingOptions`](https://docs.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.formatting.csharpformattingoptions?view=roslyn-dotnet) (use `"csharp.<property name goes here>": <value goes here>` in the configuration file).

It does not seem like Roslyn supports any VB specific configuration.
