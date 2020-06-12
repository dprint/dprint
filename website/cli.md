# Command Line Interface (CLI)

```bash
dprint <SUBCOMMAND> [OPTIONS] [--] [files]...
```

## Installing and Setup/Initialization

See [Install](install) and [Setup](setup).

## Help

The information outlined here will only be for the latest version, so `dprint help` or `dprint --help` will output information on how to use the CLI.

## Formatting Files

After [setting up a configuration file](setup), run the `fmt` command:

```bash
dprint fmt
```

Or to override the configuration file's `includes` and `excludes`, you may specify the file paths to format or not format here:

```bash
dprint fmt **/*.js --excludes **/data
```

## Checking What Files Aren't Formatted

Instead of formatting files, you can get a report of any files that aren't formatted by running:

```bash
dprint check
```

## Using a Custom Config File Path or URL

Instead of the default path of *dprint.config.json* or *config/dprint.config.json*, you can specify a path to a configuration file via the `--config` or `-c` flag.

```bash
dprint fmt --config path/to/my/config.json
```

## Diagnostic Commands and Flags

### Outputting file paths

Sometimes you may not be sure what files Dprint is picking up and formatting. To check, use the `output-file-paths` subcommand to see all the resolved file paths for the current plugins based on the CLI arguments and configuration.

```bash
dprint output-file-paths
```

Example output:

```bash
TODO
```

### Outputting resolved configuration

When diagnosing configuration issues it might be useful to find out what the internal lower level configuration used by the plugins is. To see that, use the following command:

```bash
dprint output-resolved-config
```

Example output:

```bash
TODO
```

### Verbose

It is sometimes useful to see what's going on under the hood. For those cases, run dprint with the `--verbose` flag.

For example:

```bash
dprint check --verbose
```

Example output:

```bash
TODO: Put example output here.
```

This can be exceptionally useful for finding files that are taking a long time to format and maybe can be excluded from formatting.
