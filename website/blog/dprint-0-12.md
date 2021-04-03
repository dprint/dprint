---
title: dprint 0.12
description: Overview of the new features in dprint 0.12
publish_date: 2021-04-03
author: David Sherret
---

dprint is a pluggable, configurable, and fast code formatting platform written in Rust.

This post outlines what's new in dprint 0.12.

Issues: https://github.com/dprint/dprint/milestone/6?closed=1

## _.dprintrc.json_ -> _dprint.json_

After some very helpful [feedback](https://github.com/dprint/dprint/issues/342), the default configuration file path has changed to _dprint.json_

Please rename your configuration file accordingly as support for _.dprintrc.json_ by default will be dropped in a future version of dprint.
