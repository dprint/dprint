---
title: Rustfmt Plugin
description: Documentation on the rustfmt code formatting plugin for dprint.
---

<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/plugins">Plugins</a></li>
    <li><a href="/plugins/rustfmt">Rustfmt</a></li>
  </ul>
</nav>

# Rustfmt Plugin

Wrapper plugin that formats Rust code via [rustfmt](https://github.com/rust-lang/rustfmt).

<div class="message is-warning">
  <div class="message-body">
    This is a process plugin. Using this will cause the CLI to download, run, and communicate with a separate process that is not sandboxed (unlike Wasm plugins).
  </div>
</div>

## Install and Setup

Follow the instructions at [https://github.com/dprint/dprint-plugin-rustfmt/releases/](https://github.com/dprint/dprint-plugin-rustfmt/releases/)

## Configuration

See documentation [here](https://rust-lang.github.io/rustfmt/).
