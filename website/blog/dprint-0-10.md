---
title: dprint 0.10
description: Overview of the new features in dprint 0.10
publish_date: 2020-12-02
author: David Sherret
---

dprint is a pluggable, configurable, and fast code formatting platform written in Rust.

This post outlines what's new in dprint 0.10.

Issues: https://github.com/dprint/dprint/milestone/5?closed=1

## `dprint fmt --stdin <file-path/file-name>`

Previously there was an unstable hidden `dprint stdin-fmt` command. This was removed and now there is a new `--stdin` flag on `dprint fmt` with added functionality.

This flag does what you might expect and allows you to provide text via stdin, which is then outputted to stdout.

### Providing a file path

When you provide a file path (ex. `/home/david/dev/project/src/my-file.ts`), the CLI will check the dprint configuration file to see if the file path should be formatted. If it should, it formats it. If not, it returns back the file text as-is.

### Providing a file name

If you provide only a file name (ex. `my-file.ts`) then the CLI will always format the file if it matches a plugin.

## Process Plugin Interface Change

The interface between process plugins and the CLI has changed in order to be more robust. Previously if a process plugin had a problem while sending the formatted file text back to the CLI and the error output was longer than the rest of the formatted file text, then this error message would end up in the file. Now all messages must end with "success bytes" to signify the end of a successfully sent message. Read more [here](https://github.com/dprint/dprint/blob/main/docs/process-plugin-development.md).

Due to this change, process plugins must now be above the following versions in order to work with dprint 0.10.0:

```json
{
  "plugins": [
    "https://plugins.dprint.dev/prettier-0.2.0.exe-plugin@5fc84274198107b5464477803eea335ab3c978738ff40058294b105d2e32d5ae",
    "https://plugins.dprint.dev/roslyn-0.2.2.exe-plugin@44d9f8fc4db50b07196672e857dc0244f3f274374db0d188df464dc0aae487c7",
    "https://plugins.dprint.dev/yapf-0.2.0.exe-plugin@14c42b703709e81f813c6674a8110c522af0ea78b6298f4f73721121a1a03701"
  ]
}
```

## Under the Hood

This release mostly has changes "under the hood" that should make it faster and work better.

### Custom Thread Pool

Internally, dprint now uses a custom thread pool that optimizes for minimizing plugin creation time. Additionally, if you have many CPUs, you should notice a huge performance improvement as dprint will now utilize all your CPUs.

### Custom Progress Bars and Input Selection

The input selection and progress bars have been written from scratch. The progress bars should no longer flicker on certain platforms and it now handles dprint plugins outputting while the CLI is doing other output such as displaying a progress bar.
