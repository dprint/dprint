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

Or to override the configuration file's `includes` and `excludes`, you may specify the file paths to format or not format here:

```sh
dprint fmt **/*.js --excludes **/data
```

### Formatting Standard Input

Use `dprint fmt --stdin <file-path/file-name/extension>` and provide the input file text to stdin. The output will be directed by the CLI to stdout.

Provide a full file path to format with inclusion/exclusion rules of your dprint configuration file or provide only a file name or extension to always format the file.

## Checking What Files Aren't Formatted

Instead of formatting files, you can get a report of any files that aren't formatted by running:

```sh
dprint check
```

Example output:

![Example of dprint check output.](/images/check-example.png "Example of dprint check output.")

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

## Exit codes

- `0` - Success
- `1` - General error
- `10` - Argument parsing error
- `11` - Configuration resolution error
- `12` - Plugin resolution error
- `13` - No plugins found error
- `14` - No files found error (useful for pre-commit hooks)
- `20` - `dprint check` found non-formatted files

## Diagnostic Commands and Flags

### Outputting file paths

Sometimes you may not be sure what files dprint is picking up and formatting. To check, use the `output-file-paths` subcommand to see all the resolved file paths for the current plugins based on the CLI arguments and configuration.

```sh
dprint output-file-paths
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
dprint output-resolved-config
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

### Outputting format times

It can be useful to know what files take a long time to format as you may consider skipping them. To see this information, use the following command:

```sh
dprint output-format-times
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

### Verbose

It is sometimes useful to see what's going on under the hood. For those cases, run dprint with the `--verbose` flag.

For example:

```sh
dprint check --verbose
```

Example output:

```text
[VERBOSE] Getting cache directory.
[VERBOSE] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\cache-manifest.json
[VERBOSE] Checking path exists: ./dprint.json
[VERBOSE] Reading file: V:\dev\my-project\dprint.json
[VERBOSE] Globbing: ["**/*.{ts,tsx,js,jsx,json}", "!website/playground/build", "!scripts/build-website", "!**/dist", "!**/target", "!**/wasm", "!**/*-lock.json", "!**/node_modules"]
[VERBOSE] Finished globbing in 12ms
[VERBOSE] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\typescript-0.19.2.compiled_wasm
[VERBOSE] Reading file: C:\Users\user\AppData\Local\Dprint\Dprint\cache\json-0.4.1.compiled_wasm
[VERBOSE] Creating instance of dprint-plugin-typescript
[VERBOSE] Creating instance of dprint-plugin-jsonc
[VERBOSE] Created instance of dprint-plugin-jsonc in 9ms
[VERBOSE] Reading file: V:\dev\my-project\website\playground\tsconfig.json
[VERBOSE] Reading file: V:\dev\my-project\website\assets\schemas\v0.json
[VERBOSE] Reading file: V:\dev\my-project\dprint.json
[VERBOSE] Formatted file: V:\dev\my-project\website\assets\schemas\v0.json in 2ms
[VERBOSE] Formatted file: V:\dev\my-project\dprint.json in 0ms
[VERBOSE] Formatted file: V:\dev\my-project\website\playground\tsconfig.json in 0ms
[VERBOSE] Created instance of dprint-plugin-typescript in 35ms
[VERBOSE] Reading file: V:\dev\my-project\website\playground\public\formatter.worker.js
[VERBOSE] Reading file: V:\dev\my-project\website\assets\formatter\v1.js
[VERBOSE] Reading file: V:\dev\my-project\website\playground\src\plugins\getPluginInfo.ts
[VERBOSE] Formatted file: V:\dev\my-project\website\playground\public\formatter.worker.js in 22ms
[VERBOSE] Formatted file: V:\dev\my-project\website\assets\formatter\v1.js in 6ms
[VERBOSE] Formatted file: V:\dev\my-project\website\playground\src\plugins\getPluginInfo.ts in 4ms
...etc....
```

This may be useful for finding files that are taking a long time to format and maybe should be excluded from formatting.

### Clearing Cache

Internally, a cache is used to avoid re-downloading files. It may be useful in some scenarios to clear this cache by running:

```sh
dprint clear-cache
```
