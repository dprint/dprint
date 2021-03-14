---
title: CI
description: Documentation for running dprint on continuous integration.
---

# Continuous Integration (CI)

You may desire to run `dprint check` as a step on your CI to ensure the code is formatted.

## GitHub Action

See `dprint/check`: https://github.com/marketplace/actions/dprint-check-action

## Others

It is easy to get dprint working on a CI by running the install script then `dprint check`.

For example:

```bash
curl -fsSL https://dprint.dev/install.sh | sh -s 0.11.1 > /dev/null 2>&1
$HOME/.dprint/bin/dprint check
```
