---
title: Configuration - Shell
description: Documentation on the configuration file for the Shell code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/sh">Shell</a></li>
    <li><a href="/plugins/sh/config">Configuration</a></li>
  </ul>
</nav>

# Shell - Configuration

Specify configuration under the `"sh"` key in your dprint configuration file.

| Property           | Values                                           | Default                      | Description                                                              |
| ------------------ | ------------------------------------------------ | ---------------------------- | ------------------------------------------------------------------------ |
| `dialect`          | `"auto"`, `"bash"`, `"posix"`, `"mksh"`, `"zsh"` | `"auto"`                     | Shell dialect to parse the file as.                                      |
| `indentWidth`      | Integer                                          | Global `indentWidth`, or `8` | Number of spaces for an indent.                                          |
| `useTabs`          | Boolean                                          | Global `useTabs`, or `true`  | Indent with tabs.                                                        |
| `binaryNextLine`   | Boolean                                          | `false`                      | Put binary operators (`&&`, `\|\|`, `\|`) at the start of the next line. |
| `switchCaseIndent` | Boolean                                          | `false`                      | Indent `case` pattern arms under the `case` statement.                   |
| `spaceRedirects`   | Boolean                                          | `false`                      | Put a space between a redirect operator and its target.                  |
| `keepPadding`      | Boolean                                          | `false`                      | Keep column alignment spaces between tokens.                             |
| `functionNextLine` | Boolean                                          | `false`                      | Put a function's opening brace on a new line.                            |
| `neverSplit`       | Boolean                                          | `false`                      | Keep statements on a single line where possible.                         |
| `simplify`         | Boolean                                          | `false`                      | Rewrite redundant syntax to simpler forms (ex. `${foo}` to `$foo`).      |
| `minify`           | Boolean                                          | `false`                      | Remove comments and extra whitespace.                                    |

## Indentation and Global Configuration

The plugin inherits the global `indentWidth` and `useTabs` settings. If your global configuration uses spaces for other languages and you want shell scripts to keep tabs, override it under the `"sh"` key:

```json
{
  "indentWidth": 2,
  "useTabs": false,
  "sh": {
    "useTabs": true
  }
}
```

## Dialect Resolution

When `dialect` is `"auto"`, the dialect is chosen in this order:

1. The shebang line (ex. `#!/usr/bin/env bash` is Bash).
2. The file extension: `.bash` and `.bats` are Bash, `.zsh` is Zsh, `.mksh` is mksh, and `.sh`, `.dash`, and `.ksh` are POSIX.
3. Known Zsh dotfiles (ex. `.zshrc`) are Zsh, and other files without an extension (ex. `.envrc`) are Bash.

A `.sh` file without a shebang is parsed as POSIX, so Bash syntax such as `[[ ]]` will fail to parse. Set `"dialect": "bash"` or add a Bash shebang line to fix this.

See the [plugin documentation](https://github.com/kachick/dprint-plugin-sh#configuration) and
[JSON schema](https://plugins.dprint.dev/kachick/sh/latest/schema.json) for details.
