---
title: Configuration - KDL
description: Documentation on the configuration file for the KDL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/kdl">KDL</a></li>
    <li><a href="/plugins/kdl/config">Configuration</a></li>
  </ul>
</nav>

# KDL - Configuration

Specify configuration under the `"kdl"` key in your dprint configuration file.

| Property     | Values         | Default |
| ------------ | -------------- | ------- |
| `kdlVersion` | `"v1"`, `"v2"` | `"v2"`  |

## Mixed v1 and v2 Files

Some applications still use KDL v1. If your repository has a mix of KDL versions, add a version marker to the first line of each file:

```kdl
/- kdl-version 1
simplified_ui true
```

```kdl
/- kdl-version 2
simplified_ui #true
```

When the marker is on the first line, the plugin formats the file with that version even if `kdlVersion` says otherwise.

See the [plugin documentation](https://github.com/kachick/dprint-plugin-kdl#configuration) and
[JSON schema](https://plugins.dprint.dev/kachick/kdl/latest/schema.json) for details.
