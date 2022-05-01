---
title: CI
description: Documentation for running dprint on continuous integration.
layout: layouts/documentation.njk
---

# Continuous Integration (CI)

You may desire to run `dprint check` as a step on your CI to ensure the code is formatted.

## GitHub Action

See `dprint/check`: https://github.com/marketplace/actions/dprint-check-action

## GitLab

See `dprint-ci`: https://gitlab.com/midnightexigent/dprint-ci

## Others

It is easy to get dprint working on a CI by running the install script then `dprint check`.

For example:

```sh
# replace X.X.X with the version of dprint to use
curl -fsSL https://dprint.dev/install.sh | sh -s X.X.X > /dev/null 2>&1
$HOME/.dprint/bin/dprint check
```
