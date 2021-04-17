---
title: dprint 0.13
description: Overview of the new features in dprint 0.13
publish_date: 2021-04-17
author: David Sherret
---

dprint is a pluggable, configurable, and fast code formatting platform written in Rust.

This post outlines what's new in dprint 0.13.

Issues: https://github.com/dprint/dprint/milestone/7?closed=1

## Hidden Configuration File Support

To allow developers to make a choice between a non-hidden and hidden configuration file, _.dprint.json_ is now supported in addition to _dprint.json_ for a filename the CLI and editor plugins will automatically pick up.

## Proxy Support

dprint now will look at the `HTTPS_PROXY` and `HTTP_PROXY` environment variables for determining whether to use a proxy.
