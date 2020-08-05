# Developing an Editor Extension

There are two hidden sub commands that are of use.

## `dprint editor-info`

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

## `dprint editor-fmt`

This hidden subcommand can be used to format the text provided via stdin.

1. Run specifying the `--file-path` flag (this should be the full file path):

       ```
       dprint editor-fmt --file-path file.ts
       ```
2. Then provide the text to stdin.
