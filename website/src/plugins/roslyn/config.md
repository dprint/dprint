---
title: Configuration - Roslyn
description: Documentation on the configuration file for the Roslyn code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Roslyn - Configuration

The `csharp.`-prefixed properties map to Roslyn's [`CSharpFormattingOptions`](https://docs.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.csharp.formatting.csharpformattingoptions?view=roslyn-dotnet). Roslyn does not seem to support any Visual Basic specific configuration beyond the indentation and newline settings.

<div class="plugin-config-table" data-plugin="dprint/dprint-plugin-roslyn">
  <a href="https://plugins.dprint.dev/dprint/dprint-plugin-roslyn/latest/schema.json">View the configuration schema.</a>
</div>
