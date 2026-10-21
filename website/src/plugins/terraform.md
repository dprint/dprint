---
title: Terraform Plugin
description: Documentation on the Terraform, OpenTofu, and HCL code formatting plugin for dprint.
layout: layouts/documentation.njk
---

# Terraform Plugin

Wrapper plugin that formats Terraform, OpenTofu, and HCL files. It runs [HCL](https://github.com/hashicorp/hcl)'s own code compiled to Wasm, along with the formatting logic of `tofu fmt` from [OpenTofu](https://github.com/opentofu/opentofu).

Formats .tf, .tofu, .tfvars, .tftest.hcl, and .tofutest.hcl files exactly like `tofu fmt`, and other .hcl files (ex. Packer, Nomad, and Terragrunt configuration) like `hclfmt`. To leave some .hcl files alone, add them to `excludes` in your dprint config.

## Install and Setup

In your project's directory with a dprint.json file, run:

```shellsession
dprint add terraform
```

This will update your config file to have an entry for the plugin.

```json
{
  "plugins": [
    "npm:@dprint/terraform@x.x.x"
  ]
}
```

## Configuration

There's nothing to configure, since `tofu fmt` and `terraform fmt` have no style options. Like them, the plugin keeps each file's newlines as they are.

## Source

See https://github.com/dprint/dprint-plugin-terraform
