# Developing a Dprint Editor Extension

There are two hidden sub commands that are of use.

## `dprint editor-info`

```
dprint editor-info
```

Outputs:

```
{
    "schemaVersion": 1,
    "plugins":[{
        "name": "test-plugin",
        "fileExtensions": ["txt"]
    }]
}
```

1. If the `schemaVersion` number is less than the expected, output a message saying they need to update their global `dprint` version.
2. If the `schemaVersion` number is greater than the expected, output a message saying the editor extension is not compatible and they may need to update their editor extension to the latest version.

## `dprint stdin-fmt`

This hidden subcommand can be used to format the text provided stdin.

1. Run specifying the `--file-name` flag (this is the file name only and not the full path):

    ```
    dprint stdin-fmt --file-name file.ts
    ```
2. Then provide the text to stdin.
