---
title: CLI
description: Documentation on dprint's command-line interface (CLI).
layout: layouts/documentation.njk
---

# Command Line Interface (CLI)

```sh
dprint <SUBCOMMAND> [OPTIONS] [--] [files]...
```

## Installing and Setup/Initialization

See [Install](/install) and [Setup](/setup).

## Upgrade

In versions >= 0.30, you can upgrade to the latest version by running `dprint upgrade`.

## Help

The information outlined here will only be for the latest version, so `dprint help` or `dprint help <SUBCOMMAND>` (ex. `dprint help fmt`) will output information on how to use the CLI and give more detail about some of the flags not mentioned here.

## Formatting Files

After [setting up a configuration file](/setup), run the `fmt` command:

```sh
dprint fmt
```

To format a subset of the files the configuration file matches, you may specify the file paths to format or not format:

```sh
dprint fmt **/*.js --excludes **/data
```

A rare use case, but to override/ignore the patterns in the config file, use the `--includes-override` and `--excludes-override` flags:

```sh
dprint fmt --includes-override **/*.js --excludes-override **/data
```

### Formatting only git staged files

Requires dprint >= 0.47.0

To format only files that are staged use the `--staged` flag:

```sh
dprint fmt --staged
```

Note: This requires that [git](https://git-scm.com/) is installed and that you use git for source control.

### Ignoring .gitignore

By default, dprint respects `.gitignore` files (as well as a repository's `.git/info/exclude` file) and excludes any gitignored files from formatting. To disable this behaviour, use the `--no-gitignore` flag:

```sh
dprint fmt --no-gitignore
```

### Respecting a global .gitignore

By default, dprint does not respect git's global excludes file (`core.excludesFile`, defaulting to `$XDG_CONFIG_HOME/git/ignore`). This is opt-in because it's specific to your machine and won't exist on other machines or CI, so enabling it could cause formatting results to differ between environments.

To opt in, set the `DPRINT_GLOBAL_GITIGNORE` environment variable to `1`:

```sh
DPRINT_GLOBAL_GITIGNORE=1 dprint fmt
```

The global excludes file has the lowest precedence, so a repository's `.gitignore` or `.git/info/exclude` can re-include files it ignores. Using `--no-gitignore` disables it along with all other gitignore handling.

### Formatting Standard Input

Use `dprint fmt --stdin <file-path/file-name/extension>` and provide the input file text to stdin. The output will be directed by the CLI to stdout.

Provide a full file path to format with inclusion/exclusion rules of your dprint configuration file or provide only a file name or extension to always format the file.

### Formatting a list of files from Standard Input

Requires dprint >= 0.55.0

Use the `--stdin-files` flag to read a newline-separated list of file paths to format from stdin instead of passing them as command line arguments. This is useful when piping the output of another tool into dprint:

```sh
generate_files | dprint fmt --stdin-files
```

Unlike piping through `xargs`, this handles file paths containing spaces since the only delimiter is the newline (blank lines are ignored). It also avoids the command line length limits that apply when passing many paths as arguments.

The paths are resolved against the inclusion/exclusion rules of your dprint configuration file, the same way file patterns passed on the command line are. This flag is also available for the `check`, `file-paths`, and `format-times` subcommands.

## Checking What Files Aren't Formatted

Instead of formatting files, you can get a report of any files that aren't formatted by running:

```sh
dprint check
```

Example output:

![Example of dprint check output.](/images/check-example.png "Example of dprint check output.")

### List file paths only

Use the `--list-different` flag to display only the file paths that aren't formatted.

```sh
dprint check --list-different
```

### `--fail-fast` (dprint 0.51+)

Instead of checking every file, you can have the CLI stop on the first failure:

```sh
dprint check --fail-fast
```

## Incremental Formatting

By default, dprint will only format files that have changed since the last time you formatted the code in order to drastically improve performance.

If you want to disable this functionality, you may specify `--incremental=false` on the CLI:

```sh
dprint fmt --incremental=false
```

Alternatively, specify the following in your dprint configuration file:

```json
{
  "incremental": false
  // etc...
}
```

## Using a Custom Config File Path or URL

Instead of the default dprint configuration paths you may specify a path to a configuration file via the `--config` or `-c` flag.

```sh
dprint fmt --config path/to/my/config.json
# or specify a URL
dprint fmt --config https://dprint.dev/path/to/some/config.json
```

This flag is more useful for one-off commands. It is recommended to use the default configuration file location and name as that will lead to a better user experience.

## Changing Config Discovery

Starting in dprint 0.50, you can change the way dprint discovers configuration files by using the `--config-discovery` flag:

- `--config-discovery=default` (default) - Discovers configuration files in the current directory, ancestor directories, and descendant directories while searching for files to format.
- `--config-discovery=ignore-descendants` - Discovers configuration files in the current directory and ancestor directories only.
- `--config-discovery=global` - Use the global config file only (dprint 0.51+)
- `--config-discovery=false` - Disables all configuration discovery (specify either `--config=<path>` or `--plugins <url-or-path>`).

Note this can also be set via the `DPRINT_CONFIG_DISCOVERY` environment variable (ex. `DPRINT_CONFIG_DISCOVERY=false`, `DPRINT_CONFIG_DISCOVERY=global`, etc.)

## Coloured Output

By default, dprint colours its output (for example, the diffs shown by `dprint check` and `dprint fmt --diff`). Unlike many tools, it emits colours regardless of whether the output is a terminal, so colours show up in CI logs as well. This is controlled by two environment variables:

- `NO_COLOR` - Set to any non-empty value to disable coloured output. See [no-color.org](https://no-color.org/).
- `FORCE_COLOR` - Set to any non-empty value to force coloured output on, even when `NO_COLOR` is set. Takes precedence over `NO_COLOR`.

```sh
# disable colours
NO_COLOR=1 dprint check

# re-enable colours in an environment that sets NO_COLOR
FORCE_COLOR=1 dprint check
```

## Exit codes

- `0` - Success
- `1` - General error
- `10` - Argument parsing error
- `11` - Configuration resolution error
- `12` - Plugin resolution error
- `13` - No plugins found error
- `14` - No files found error (or suppress to `0` with `--allow-no-files` in dprint >= 0.43)
- `20` - `dprint check` found non-formatted files, or `dprint fmt --fail-on-change` formatted files

## Shell completions

Shell completions can be generated by running `dprint completions <shell>`.

Supported shells:

- `bash`
- `elvish`
- `fish`
- `powershell`
- `zsh`

Example (bash):

```sh
dprint completions bash > /usr/local/etc/bash_completion.d/dprint.bash
source /usr/local/etc/bash_completion.d/dprint.bash
```

## Diagnostic Commands and Flags

### Outputting file paths

Sometimes you may not be sure what files dprint is picking up and formatting. To check, use the `file-paths` subcommand to see all the resolved file paths for the current plugins based on the CLI arguments and configuration.

```sh
dprint file-paths
```

Example output:

```sh
C:\dev\my-project\scripts\build-homepage.js
C:\dev\my-project\scripts\build-schemas.js
C:\dev\my-project\website\playground\config-overrides.js
C:\dev\my-project\website\playground\src\components\ExternalLink.tsx
C:\dev\my-project\website\playground\src\components\index.ts
C:\dev\my-project\website\playground\src\components\Spinner.tsx
...etc...
```

### Outputting resolved configuration

When diagnosing configuration issues it might be useful to find out what the internal lower level configuration used by the plugins is. To see that, use the following command:

```sh
dprint resolved-config
```

Example output (JSON):

```json
{
  "typescript": {
    "arguments.preferHanging": true,
    "arguments.preferSingleLine": false,
    "arguments.trailingCommas": "onlyMultiLine",
    "arrayExpression.preferHanging": true,
    "arrayExpression.preferSingleLine": false,
    "arrayExpression.trailingCommas": "onlyMultiLine",
    "arrayPattern.preferHanging": true,
    // ...etc...
    "whileStatement.singleBodyPosition": "nextLine",
    "whileStatement.spaceAfterWhileKeyword": true,
    "whileStatement.useBraces": "preferNone"
  },
  "json": {
    "commentLine.forceSpaceAfterSlashes": true,
    "indentWidth": 2,
    "lineWidth": 160,
    "newLineKind": "lf",
    "useTabs": false
  }
}
```

Optionally use the `--file` flag to limit the output to only the plugins that would format that file. This is useful for verifying which plugins are actually associated with a file:

```sh
dprint resolved-config --file path/to/file.py
```

### Outputting format times

It can be useful to know what files take a long time to format as you may consider skipping them. To see this information, use the following command:

```sh
dprint format-times
```

Example output:

```text
0ms - C:\dev\my-project\dprint.json
0ms - C:\dev\my-project\README.md
1ms - C:\dev\my-project\other-file.md
2ms - C:\dev\my-project\package.json
2ms - C:\dev\my-project\my-markdown-file.md
4ms - C:\dev\my-project\test.ts
5ms - C:\dev\my-project\docs\info.md
16ms - C:\dev\my-project\my-file.ts
46ms - C:\dev\my-project\docs\overview.md
54ms - C:\dev\my-project\build.js
```

### Outputting incremental state (advanced, for very large repositories)

When [incremental formatting](#incremental-formatting) is enabled, dprint keeps a cache and reuses it as long as nothing that affects formatting output has changed. The whole cache for a configuration file is thrown away when its plugins, plugin versions, resolved configuration, associations, overrides, or global configuration change.

The `incremental-state` subcommand prints the exact signal dprint uses to make that decision, so you can compare it between two revisions and find out ahead of time whether the cache would be reused or invalidated:

```sh
dprint incremental-state
```

Example output (JSON):

```json
{
  "configs": [
    {
      "path": "/home/david/dev/my-project/dprint.json",
      "hash": "3f9c2a7b1e4d8f60",
      "plugins": [
        { "name": "dprint-plugin-typescript", "version": "0.95.15" },
        { "name": "dprint-plugin-json", "version": "0.21.1" }
      ]
    }
  ]
}
```

A `configs` entry is emitted for every configuration file dprint discovers (including descendant configuration files), each with the `hash` that gates its incremental cache. The `hash` is the only thing that matters for cache invalidation—the `plugins` list is included so the output is easy to inspect when a hash changes. The output is deterministic across machines for an unchanged configuration.

This is useful in CI to avoid formatting the entire repository when only a few files changed. For example, format only the files in the changeset when the cache would survive, and otherwise fall back to formatting everything:

```sh
git checkout main
previous_state=$(dprint incremental-state)
git checkout "$BRANCH"
new_state=$(dprint incremental-state)

if [ "$previous_state" != "$new_state" ]; then
  # plugins or configuration changed—the cache is invalid, so format everything
  dprint fmt
else
  # the cache is still valid, so only format the changed files
  dprint fmt $(git diff --name-only main...)
fi
```

### Log Level

To adjust your logging level, use the `--log-level` flag (defaults to `--log-level=info`).

- `silent` - Outputs nothing.
- `error` - Outputs fatal error messages.
- `warn` - Additionally outputs warnings.
- `info` - Additionally outputs informational messages.
- `debug` - Additionally outputs debug messages.

#### Debug Logging

Take note that `--log-level=debug` is very useful to see what's going on under the hood.

For example:

```sh
dprint check --log-level=debug
```

Example output:

```text
[DEBUG] Getting cache directory.
[DEBUG] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\cache-manifest.json
[DEBUG] Checking path exists: ./dprint.json
[DEBUG] Reading file: V:\dev\my-project\dprint.json
[DEBUG] Globbing: ["**/*.{ts,tsx,js,jsx,json}", "!website/playground/dist", "!scripts/build-website", "!**/dist", "!**/target", "!**/wasm", "!**/*-lock.json", "!**/node_modules"]
[DEBUG] Finished globbing in 12ms
[DEBUG] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\typescript-0.19.2.compiled_wasm
[DEBUG] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\json-0.4.1.compiled_wasm
[DEBUG] Creating instance of dprint-plugin-typescript
[DEBUG] Creating instance of dprint-plugin-jsonc
[DEBUG] Created instance of dprint-plugin-jsonc in 9ms
[DEBUG] Reading file: V:\dev\my-project\website\playground\tsconfig.json
[DEBUG] Reading file: V:\dev\my-project\website\assets\schemas\v0.json
[DEBUG] Reading file: V:\dev\my-project\dprint.json
[DEBUG] Formatted file: V:\dev\my-project\website\assets\schemas\v0.json in 2ms
[DEBUG] Formatted file: V:\dev\my-project\dprint.json in 0ms
[DEBUG] Formatted file: V:\dev\my-project\website\playground\tsconfig.json in 0ms
[DEBUG] Created instance of dprint-plugin-typescript in 35ms
[DEBUG] Reading file: V:\dev\my-project\website\playground\public\formatter.worker.js
[DEBUG] Reading file: V:\dev\my-project\website\assets\formatter\v1.js
[DEBUG] Reading file: V:\dev\my-project\website\playground\src\plugins\getPluginInfo.ts
[DEBUG] Formatted file: V:\dev\my-project\website\playground\public\formatter.worker.js in 22ms
[DEBUG] Formatted file: V:\dev\my-project\website\assets\formatter\v1.js in 6ms
[DEBUG] Formatted file: V:\dev\my-project\website\playground\src\plugins\getPluginInfo.ts in 4ms
...etc....
```

This may be useful for finding files that are taking a long time to format and maybe should be excluded from formatting.

### Clearing Cache

Internally, a cache is used to avoid re-downloading files. It may be useful in some scenarios to clear this cache by running:

```sh
dprint clear-cache
```
