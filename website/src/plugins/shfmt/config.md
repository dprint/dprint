---
title: Configuration - shfmt
description: Documentation on the configuration file for the shfmt (shell script) code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# shfmt - Configuration

The options are shfmt's formatting flags. `indentWidth` and `useTabs` use the global configuration when not specified, and otherwise default to shfmt's tabs. With a `dialect` of `"auto"`, the dialect is picked like the shfmt CLI picks it: from the file name, then the shebang, falling back to Bash.

See the [plugin documentation](https://github.com/dprint/dprint-plugin-shfmt#configuration) for more details.

<div class="plugin-config-table" data-plugin="dprint/dprint-plugin-shfmt">
  <a href="https://plugins.dprint.dev/dprint/dprint-plugin-shfmt/latest/schema.json">View the configuration schema.</a>
</div>
