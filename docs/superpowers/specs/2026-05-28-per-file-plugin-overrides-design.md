# Per-File Plugin Overrides Design

## Context

Issue dprint/dprint#996 asks for plugin configuration values to be overridden for
specific files. The motivating case is formatting JSON files such as
`package.json` and `composer.json` differently from other JSON files. Later
discussion also mentions JSON-like Sublime Text files and TypeScript settings
for Svelte files.

dprint already has per-format `override_config` plumbing for wasm and process
plugins. The missing behavior is CLI-owned config parsing, glob matching, and
choosing override values before each format call.

## Goals

- Allow plugin-scoped config overrides selected by file glob.
- Keep the issue's single-object syntax valid.
- Also support an array form for multiple override groups.
- Apply overrides consistently across CLI formatting, stdin formatting, LSP,
  editor service formatting, and host formatting between plugins.
- Preserve existing file routing semantics: `overrides` changes config only.
- Match existing bad-config behavior for structural errors, diagnostics, and
  locked inherited configuration.

## Non-Goals

- Do not add root-level or global per-file overrides.
- Do not make `overrides` select files for formatting.
- Do not change the plugin protocol or require plugin updates.
- Do not add named profiles or broader monorepo config routing.

## Public Config Shape

`overrides` is a reserved property inside a plugin configuration object. It may
be a single object:

```jsonc
{
  "json": {
    "overrides": {
      "files": ["**/package.json", "**/composer.json"],
      "indentWidth": 4,
      "useTabs": false
    }
  }
}
```

or an array:

```jsonc
{
  "json": {
    "overrides": [
      {
        "files": ["**/package.json"],
        "indentWidth": 4
      },
      {
        "files": ["**/composer.json"],
        "useTabs": false
      }
    ]
  }
}
```

Each override object must have `files`, either a string or an array of strings.
All other properties in that object are plugin configuration values applied only
when the file path matches.

If more than one override block matches a file, blocks are merged in declaration
order and later values win. Request-time overrides, such as overrides sent by an
editor or plugin host-format request, are merged last and win over config-file
overrides.

## File Routing Semantics

`overrides` does not affect file discovery or plugin routing. A file must still:

1. be selected by `includes`/`excludes` and CLI file patterns, and
2. be routed to the plugin by extension, file name, or `associations`.

Users who need to format extra file names or extensions with a plugin should use
`associations`. Users who need different config for files already handled by a
plugin should use `overrides`.

## Internal Model

Add a raw parsed shape beside the existing plugin config metadata:

```rust
pub struct RawPluginConfigOverride {
  pub files: Vec<String>,
  pub properties: ConfigKeyMap,
}

pub struct RawPluginConfig {
  pub associations: Option<Vec<String>>,
  pub locked: bool,
  pub overrides: Vec<RawPluginConfigOverride>,
  pub properties: ConfigKeyMap,
}
```

At runtime, resolve override file patterns into matchers stored on
`PluginWithConfig`, for example:

```rust
pub struct PluginConfigOverride {
  pub file_matcher: GlobMatcher,
  pub properties: ConfigKeyMap,
}
```

The exact struct names may vary, but the boundary should remain the same:
deserialization owns raw config shape, plugin resolution owns compiled matchers,
and formatting only asks for the override map for a concrete path.

## Config Parsing And Inheritance

`deserialize_config.rs` should reserve `overrides` the same way it reserves
`locked` and `associations`. The property must not be forwarded to plugins as an
unknown plugin option.

Structural errors fail as config errors:

- `overrides` is not an object or array of objects.
- an override object has no `files` property.
- `files` is not a string or array of strings.
- `files` contains a non-string entry.
- an override block has no plugin properties after `files` is removed.

Config extension merging happens in `resolve_config.rs` next to current plugin
property and association merging. Parent/extended overrides are kept before
child/local overrides, so later local override blocks win naturally when both
match the same file.

Locked inherited plugin configuration follows existing behavior. If an extended
plugin config is locked, a child config may not add plugin properties or
per-file overrides for that plugin. The resulting error should mirror the
existing locked-config error style.

## Formatting Flow

For each file/plugin format call:

1. Determine the plugins for the file using existing plugin name resolution.
2. For each plugin, collect matching override blocks from that plugin's compiled
   matchers.
3. Merge matching block properties in declaration order.
4. Merge request-time `override_config` over those config-file overrides.
5. Pass the final override map through the existing
   `InitializedPluginWithConfigFormatRequest.override_config`.

Normal CLI formatting currently passes an empty override map from `format.rs`;
that call should instead use the plugin's config-file overrides for the file.
`PluginsScope::format` should do the same before merging any request-time
override map. This covers stdin formatting, LSP formatting, editor service
formatting, and host formatting between plugins because they already route
through `PluginsScope::format`.

The wasm and process plugin transports already merge `override_config` with the
registered plugin config before formatting, so the plugin protocol does not need
to change.

## Diagnostics And Caching

Plugin option errors inside override blocks should be reported as plugin config
diagnostics before formatting proceeds. To do this, each override block should
be validated by resolving base plugin config plus that override's properties and
checking diagnostics during plugin initialization or plugin scope resolution.

Override file patterns and properties must be included in
`PluginWithConfig::incremental_hash`. Without that, incremental formatting could
skip files after only an override changes.

`output-resolved-config` should continue to show the base resolved plugin
configuration. It does not have a file path, so it cannot show a file-specific
resolved variant.

## Documentation And Schema

Add documentation near the existing `Associations` section in
`website/src/config.md`. The docs should state:

- `overrides` is plugin-scoped.
- it changes config for matching files only.
- it does not include, exclude, or associate files.
- when multiple override blocks match, later blocks win.

Update `website/src/assets/schemas/v0.json` so editor schema validation allows
`overrides` inside plugin configuration objects.

## Test Plan

Add focused tests in the existing dprint test modules.

Config parsing:

- single-object `overrides`
- array `overrides`
- invalid `overrides` type
- missing `files`
- invalid `files` type
- `overrides` not present in plugin properties

Config inheritance:

- extended override applies
- local override follows parent override and wins on duplicate keys
- locked inherited config rejects local override entries

Formatting:

- normal files use base config
- matching files use override config
- multiple matching override blocks apply in order
- non-matching override blocks have no effect
- request-time override config wins over config-file override config

Entry points:

- `fmt` or `check` applies overrides
- `stdin-fmt` applies overrides based on the provided file name/path
- LSP formatting applies overrides
- editor service formatting applies overrides
- host formatting between plugins preserves request-time override precedence

Caching:

- changing only an override value changes the plugin incremental hash

Verification commands:

```sh
cargo test -p dprint -- configuration::deserialize_config
cargo test -p dprint -- configuration::resolve_config
cargo test -p dprint -- commands::formatting
cargo test -p dprint -- commands::lsp
cargo test -p dprint -- commands::editor
cargo test --workspace
cargo fmt --check
```
