# Developing an Editor Extension (Schema Version 2)

Editor extensions communicate with the CLI using the `dprint editor-info` and `dprint editor-service` subcommand.

## dprint editor-info

Called first in order to get information about the current working directory.

```
dprint editor-info
```

Outputs:

```
{
    "schemaVersion": 2,
    "plugins":[{
        "name": "test-plugin",
        "fileExtensions": ["txt"]
    }]
}
```

1. If the `schemaVersion` number is less than the expected, output a message saying they need to update their global `dprint` version.
2. If the `schemaVersion` number is greater than the expected, output a message saying the editor extension is not compatible and they may need to update their editor extension to the latest version.

## dprint editor-service

This starts a long running process that can be communicated with over stdin and stdout.

### Executing

Run `dprint editor-service --parent-pid <provide your current process pid here>`

The editor service polls for the provided process id every 30 seconds and if it doesn't exist it will exit.

### Message Kinds

After startup, send one of the following messages:

- `0` - Shutdown the process
- `1` - Check if a path can be formatted by the CLI.
- `2` - Format a file.

#### `0` - Shutting down the process

- Editor sends
  - u32 (4 bytes) - Message kind `1` indicating to shut down the process.

#### `1` - Checking a file can be formatted

- Editor sends:
  - u32 (4 bytes) - Message kind `2` indicating to check if a path can be formatted by the CLI.
  - u32 (4 bytes) - Path file size
  - X bytes - Path as string
- CLI responds:
  - u32 (4 bytes) - 0 for cannot format, or 1 for can format

#### `2` - Formatting a file

- Editor sends:
  - u32 (4 bytes) - Message kind `3` for formatting a file.
  - u32 (4 bytes) - Path file size
  - X bytes - Path as string
  - u32 (4 bytes) - File text size
  - X bytes - File text
- CLI responds:
  - u32 (4 bytes) - 0 for no change (END, no more messages), 1 for change, 2 for error
  - u32 (4 bytes) - Formatted file text or error message size
  - X bytes - Formatted file text or error message

### General

- Everything is big endian and utf-8
- Communication is always done with a buffer size of 1024. So if sending data (X bytes) above 1024 bytes then the following protocol happens:
  1. Write 1024 bytes.
  2. Wait for 4 byte ready response from CLI
  3. If there are still more than 1024 bytes to write, write 1024 bytes and go back to step 2. If not, write the remaining bytes and exit the loop.

If using Rust, there is a `StdInOutReaderWriter` in dprint-core that helps with this.
