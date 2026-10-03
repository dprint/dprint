use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use std::time::SystemTime;

use deno_tower_lsp::lsp_types as lsp;
use deno_tower_lsp::lsp_types::Uri;
use jsonc_parser::Scanner;
use jsonc_parser::tokens::Token;
use serde_json::Value;
use text_size::TextSize;
use tokio::sync::Notify;
use url::Url;

use crate::configuration::POSSIBLE_CONFIG_FILE_NAMES;
use crate::environment::Environment;

use super::config::LspPluginsScopeContainer;
use super::text::LineIndex;

/// The dprint configuration file JSON schema, embedded at compile time so that
/// completions for the well-known root keys work without any network access.
///
/// This crate is the source of truth for the schema. The website build copies
/// this file to `website/src/assets/schemas/v0.json` so it's also served at
/// https://dprint.dev/schemas/v0.json (see `website/_config.ts`).
const DPRINT_CONFIG_SCHEMA: &str = include_str!("config_schema.json");

/// How long a completion or hover request waits on the downloads of plugin
/// config schemas before it's answered with the schemas that are available.
/// A download is only waited on this long once, the requests after that are
/// answered without waiting on it.
const SCHEMA_DOWNLOAD_WAIT: Duration = Duration::from_secs(2);

/// How long after a plugin config schema failed to download or parse until
/// it's downloaded again.
const SCHEMA_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// Provides completions and hover information for dprint configuration files.
///
/// This is intentionally isolated from the rest of the language server: it owns
/// the base schema, fetches and caches each resolved plugin's configuration
/// schema (see [`PluginSchemas`]), then stitches them together into a
/// [`CompositeSchema`] that drives schema-aware suggestions. The actual
/// analysis ([`completions_for`] and [`hover_for`]) is pure and operates only
/// on text + a composite schema, which keeps it easy to test without a running
/// environment.
pub struct ConfigCompletions<TEnvironment: Environment> {
  scope_container: Rc<LspPluginsScopeContainer<TEnvironment>>,
  base_schema: Rc<Value>,
  plugin_schemas: PluginSchemas<TEnvironment>,
}

/// Gets whether the given uri points at a file dprint recognizes as a
/// configuration file (ex. `dprint.json`).
pub fn is_config_uri(uri: &Uri) -> bool {
  let file_name = uri.path().as_str().rsplit('/').next().unwrap_or_default();
  POSSIBLE_CONFIG_FILE_NAMES.contains(&file_name)
}

/// The completion and hover capabilities of the server, which are only for
/// dprint configuration files.
pub struct ConfigFileCapabilities {
  /// The completion capability to provide in the initialize result, which is
  /// `None` when completion is registered for only the configuration files.
  pub completion_provider: Option<lsp::CompletionOptions>,
  /// The hover capability to provide in the initialize result, which is
  /// `None` when hover is registered for only the configuration files.
  pub hover_provider: Option<lsp::HoverProviderCapability>,
  /// The registrations to send the client once it's initialized.
  pub registrations: Vec<lsp::Registration>,
}

/// Gets how the server provides completion and hover to the client. The server's
/// capabilities in the initialize result can't specify the documents they're for,
/// so they have the client request completions and hover information in every
/// document it uses the server for, which the server only has for configuration
/// files. Registering them with a document selector instead has the client only
/// send the requests for configuration files, but that's only possible in a
/// client that supports dynamically registering them.
pub fn get_config_file_capabilities(capabilities: &lsp::ClientCapabilities) -> ConfigFileCapabilities {
  let text_document = capabilities.text_document.as_ref();
  let can_register_completion = text_document.and_then(|t| t.completion.as_ref()).and_then(|c| c.dynamic_registration) == Some(true);
  let can_register_hover = text_document.and_then(|t| t.hover.as_ref()).and_then(|c| c.dynamic_registration) == Some(true);
  let completion_options = lsp::CompletionOptions {
    // `"` opens a property/value string, `:` moves to a value position
    trigger_characters: Some(vec!["\"".to_string(), ":".to_string()]),
    ..Default::default()
  };
  let text_document_registration_options = lsp::TextDocumentRegistrationOptions {
    document_selector: Some(
      POSSIBLE_CONFIG_FILE_NAMES
        .iter()
        .map(|file_name| lsp::DocumentFilter {
          language: None,
          scheme: None,
          pattern: Some(format!("**/{}", file_name)),
        })
        .collect(),
    ),
  };
  let mut result = ConfigFileCapabilities {
    completion_provider: None,
    hover_provider: None,
    registrations: Vec::new(),
  };
  let mut register = |method: &str, options: Value| {
    result.registrations.push(lsp::Registration {
      id: format!("dprint-config-{}", method),
      method: method.to_string(),
      register_options: Some(options),
    });
  };
  if can_register_completion {
    register(
      "textDocument/completion",
      serde_json::to_value(lsp::CompletionRegistrationOptions {
        text_document_registration_options: text_document_registration_options.clone(),
        completion_options,
      })
      .unwrap(),
    );
  } else {
    result.completion_provider = Some(completion_options);
  }
  if can_register_hover {
    register(
      "textDocument/hover",
      serde_json::to_value(lsp::HoverRegistrationOptions {
        text_document_registration_options,
        hover_options: Default::default(),
      })
      .unwrap(),
    );
  } else {
    result.hover_provider = Some(lsp::HoverProviderCapability::Simple(true));
  }
  result
}

impl<TEnvironment: Environment> ConfigCompletions<TEnvironment> {
  pub fn new(environment: TEnvironment, scope_container: Rc<LspPluginsScopeContainer<TEnvironment>>) -> Self {
    let base_schema = serde_json::from_str(DPRINT_CONFIG_SCHEMA).expect("dprint config schema should be valid json");
    Self {
      scope_container,
      base_schema: Rc::new(base_schema),
      plugin_schemas: PluginSchemas::new(environment, SCHEMA_DOWNLOAD_WAIT),
    }
  }

  pub async fn completions(&self, file_path: &Path, file_text: &str, position: lsp::Position, use_global_config: bool) -> Option<lsp::CompletionList> {
    let line_index = LineIndex::new(file_text);
    let offset: usize = u32::from(line_index.offset(position)) as usize;
    let schema = self.build_composite_schema(file_path, use_global_config).await;
    Some(lsp::CompletionList {
      // so the client asks again as the user types instead of filtering a
      // list that might lack what's in a schema that's still downloading
      is_incomplete: schema.is_missing_downloading_schema,
      items: completions_for(&schema, file_text, &line_index, offset),
    })
  }

  pub async fn hover(&self, file_path: &Path, file_text: &str, position: lsp::Position, use_global_config: bool) -> Option<lsp::Hover> {
    let line_index = LineIndex::new(file_text);
    let offset: usize = u32::from(line_index.offset(position)) as usize;
    let schema = self.build_composite_schema(file_path, use_global_config).await;
    hover_for(&schema, file_text, &line_index, offset)
  }

  async fn build_composite_schema(&self, file_path: &Path, use_global_config: bool) -> CompositeSchema {
    let mut plugins = Vec::new();
    let mut is_missing_downloading_schema = false;
    if let Some(parent) = file_path.parent() {
      // a parse error while the user is mid-edit just means we fall back to
      // base-schema-only completions, so ignore any resolution error here
      if let Ok(Some(scope)) = self.scope_container.resolve_by_path(parent, use_global_config).await {
        let infos = scope.plugins.values().map(|plugin| plugin.info()).collect::<Vec<_>>();
        let urls = infos.iter().map(|info| info.config_schema_url.as_str()).collect::<Vec<_>>();
        let schemas = self.plugin_schemas.get_all(&urls).await;
        is_missing_downloading_schema = self.plugin_schemas.is_any_downloading(&urls);
        for (info, schema) in infos.into_iter().zip(schemas) {
          plugins.push(PluginSchema {
            config_key: info.config_key.clone(),
            name: info.name.clone(),
            schema,
          });
        }
      }
    }
    CompositeSchema {
      base: self.base_schema.clone(),
      plugins,
      is_missing_downloading_schema,
    }
  }
}

/// Downloads the plugins' config schemas and caches them by url.
///
/// A url is only downloaded by one task at a time, which the requests that
/// need the schema wait on for a limited time. A request that gives up on
/// waiting doesn't stop the download, so a later request gets its result, but
/// the requests until then don't wait on that download again.
struct PluginSchemas<TEnvironment: Environment> {
  environment: TEnvironment,
  entries: Rc<RefCell<HashMap<String, SchemaEntry>>>,
  /// How long a request waits on the downloads.
  wait: Duration,
}

enum SchemaEntry {
  Downloading(Rc<SchemaDownload>),
  Ready(Rc<Value>),
  /// The schema failed to download or parse at this time.
  Failed(SystemTime),
}

#[derive(Default)]
struct SchemaDownload {
  /// Notifies its waiters once the entry was replaced with the result.
  finished: Notify,
  /// Whether a request waited the whole time on this download without getting
  /// its result, in which case the next requests don't wait on it.
  was_given_up_on: Cell<bool>,
}

impl<TEnvironment: Environment> PluginSchemas<TEnvironment> {
  fn new(environment: TEnvironment, wait: Duration) -> Self {
    Self {
      environment,
      entries: Default::default(),
      wait,
    }
  }

  /// Gets the schemas at the provided urls in the same order. A schema is
  /// `None` when its url is empty, it failed to download or parse, or it
  /// didn't download in time.
  async fn get_all(&self, urls: &[&str]) -> Vec<Option<Rc<Value>>> {
    // start every download before waiting on any of them, so the wait is
    // for all of them at once
    for url in urls {
      self.ensure_downloaded(url.trim());
    }
    let deadline = tokio::time::Instant::now() + self.wait;
    let mut schemas = Vec::with_capacity(urls.len());
    for url in urls {
      schemas.push(self.wait_for(url.trim(), deadline).await);
    }
    schemas
  }

  /// Gets whether the schema at any of the provided urls is being downloaded.
  fn is_any_downloading(&self, urls: &[&str]) -> bool {
    let entries = self.entries.borrow();
    urls.iter().any(|url| matches!(entries.get(url.trim()), Some(SchemaEntry::Downloading(_))))
  }

  /// Starts downloading the schema when it isn't cached or being downloaded,
  /// or when its last failure was long enough ago to try again.
  fn ensure_downloaded(&self, url: &str) {
    if url.is_empty() {
      return;
    }
    let download = Rc::new(SchemaDownload::default());
    {
      let mut entries = self.entries.borrow_mut();
      match entries.get(url) {
        Some(SchemaEntry::Downloading(_) | SchemaEntry::Ready(_)) => return,
        Some(SchemaEntry::Failed(time)) if !self.is_retry_due(*time) => return,
        Some(SchemaEntry::Failed(_)) | None => {}
      }
      entries.insert(url.to_string(), SchemaEntry::Downloading(download.clone()));
    }

    let environment = self.environment.clone();
    let entries = self.entries.clone();
    let url = url.to_string();
    dprint_core::async_runtime::spawn(async move {
      let entry = match download_schema(&environment, &url).await {
        Some(schema) => SchemaEntry::Ready(schema),
        None => SchemaEntry::Failed(environment.sys_time_now()),
      };
      entries.borrow_mut().insert(url, entry);
      download.finished.notify_waiters();
    });
  }

  fn is_retry_due(&self, failed_time: SystemTime) -> bool {
    match self.environment.sys_time_now().duration_since(failed_time) {
      Ok(elapsed) => elapsed >= SCHEMA_RETRY_INTERVAL,
      // the clock was set back
      Err(_) => true,
    }
  }

  async fn wait_for(&self, url: &str, deadline: tokio::time::Instant) -> Option<Rc<Value>> {
    let download = match self.entries.borrow().get(url) {
      // a host that stays slow only delays the first request this way
      Some(SchemaEntry::Downloading(download)) if !download.was_given_up_on.get() => Some(download.clone()),
      _ => None,
    };
    if let Some(download) = download
      && tokio::time::timeout_at(deadline, download.finished.notified()).await.is_err()
    {
      download.was_given_up_on.set(true);
      log_debug!(self.environment, "Timed out waiting on the download of the config schema at {}", url);
    }
    match self.entries.borrow().get(url) {
      Some(SchemaEntry::Ready(schema)) => Some(schema.clone()),
      _ => None,
    }
  }
}

async fn download_schema<TEnvironment: Environment>(environment: &TEnvironment, url: &str) -> Option<Rc<Value>> {
  let parsed_url = match Url::parse(url) {
    Ok(url) => url,
    Err(err) => {
      log_debug!(environment, "Failed parsing config schema url {}: {:#}", url, err);
      return None;
    }
  };
  match environment.download_file_err_404(&parsed_url, None).await {
    Ok((_, file)) => match serde_json::from_slice::<Value>(&file.content) {
      Ok(value) => Some(Rc::new(value)),
      Err(err) => {
        log_debug!(environment, "Failed parsing config schema at {}: {:#}", url, err);
        None
      }
    },
    Err(err) => {
      log_debug!(environment, "Failed downloading config schema at {}: {:#}", url, err);
      None
    }
  }
}

/// The dprint base schema combined with the resolved plugins' schemas.
struct CompositeSchema {
  base: Rc<Value>,
  plugins: Vec<PluginSchema>,
  /// Whether the schema of a plugin is missing because it's still being
  /// downloaded.
  is_missing_downloading_schema: bool,
}

struct PluginSchema {
  config_key: String,
  name: String,
  /// The plugin's configuration schema. `None` when the plugin doesn't expose
  /// one or it couldn't be downloaded.
  schema: Option<Rc<Value>>,
}

impl CompositeSchema {
  fn base_root(&self) -> SchemaRef<'_> {
    SchemaRef {
      doc: &self.base,
      node: &self.base,
    }
  }

  /// The base schema's `additionalProperties`, which describes the properties
  /// common to every plugin's config section (ex. `locked`, `associations`).
  fn base_plugin_section(&self) -> Option<SchemaRef<'_>> {
    self.base.get("additionalProperties").map(|node| SchemaRef { doc: &self.base, node })
  }

  /// The schema describing a single entry of a plugin section's `overrides`
  /// (an object with `files` plus arbitrary plugin config).
  fn base_override_item(&self) -> Option<SchemaRef<'_>> {
    override_item(self.base_plugin_section()?.property("overrides")?)
  }

  fn plugin_by_key(&self, key: &str) -> Option<&PluginSchema> {
    self.plugins.iter().find(|p| p.config_key == key)
  }

  fn plugin_root<'a>(&'a self, plugin: &'a PluginSchema) -> Option<SchemaRef<'a>> {
    plugin.schema.as_ref().map(|schema| SchemaRef { doc: schema, node: schema })
  }

  /// Resolves the object or array located at `path` into the set of schema
  /// nodes that describe it. Suggestions are the union across the set, which is
  /// how plugin sections merge the plugin's own schema with the common section
  /// properties, and how `overrides` entries merge `files` with plugin config.
  fn schema_set_for_path(&self, path: &[PathSeg]) -> SchemaSet<'_> {
    // a top level plugin config section is handled specially
    if let Some(PathSeg::Key(key)) = path.first()
      && let Some(plugin) = self.plugin_by_key(key)
    {
      return self.plugin_section_set(plugin, &path[1..]);
    }

    // otherwise navigate within the base schema
    let mut current = self.base_root();
    for seg in path {
      current = match navigate(current, seg) {
        Some(node) => node,
        None => return SchemaSet::empty(),
      };
    }
    SchemaSet::single(current)
  }

  fn plugin_section_set<'a>(&'a self, plugin: &'a PluginSchema, rest: &[PathSeg]) -> SchemaSet<'a> {
    // the plugin section object itself: the plugin's own schema plus the
    // properties common to every section (locked, associations, overrides)
    if rest.is_empty() {
      let mut refs = Vec::new();
      refs.extend(self.plugin_root(plugin));
      refs.extend(self.base_plugin_section());
      return SchemaSet { refs };
    }

    // inside `overrides`: each entry is an override object, regardless of
    // whether `overrides` was written as a single object or an array
    if matches!(&rest[0], PathSeg::Key(key) if key == "overrides") {
      let mut after = &rest[1..];
      if after.first() == Some(&PathSeg::Elem) {
        after = &after[1..];
      }
      // an override object accepts `files` plus the plugin's own config
      let mut refs = Vec::new();
      refs.extend(self.base_override_item());
      refs.extend(self.plugin_root(plugin));
      navigate_set(SchemaSet { refs }, after)
    } else {
      // a nested property of the plugin's own config
      let Some(root) = self.plugin_root(plugin) else {
        return SchemaSet::empty();
      };
      navigate_set(SchemaSet::single(root), rest)
    }
  }

  /// Collects the property names that can be suggested for the object at
  /// `path`, excluding any already present in `existing_keys`.
  fn name_options(&self, path: &[PathSeg], existing_keys: &[String]) -> Vec<NameOption> {
    let mut options: Vec<NameOption> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let push = |options: &mut Vec<NameOption>, seen: &mut Vec<String>, option: NameOption| {
      if seen.iter().any(|n| n == &option.name) || existing_keys.iter().any(|k| k == &option.name) {
        return;
      }
      seen.push(option.name.clone());
      options.push(option);
    };

    for (name, prop) in self.schema_set_for_path(path).property_names() {
      let prop = prop.deref();
      push(
        &mut options,
        &mut seen,
        NameOption {
          name,
          detail: prop.type_label(),
          documentation: prop.description().map(str::to_string),
        },
      );
    }

    // at the root, every resolved plugin's config key is a valid property
    if path.is_empty() {
      for plugin in &self.plugins {
        push(
          &mut options,
          &mut seen,
          NameOption {
            name: plugin.config_key.clone(),
            detail: Some("plugin".to_string()),
            documentation: Some(format!("Configuration for the \"{}\" plugin.", plugin.name)),
          },
        );
      }
    }

    options
  }

  /// Collects the value suggestions for the property `key` of the object at
  /// `path` (ex. the variants of an enum, or `true`/`false`). When `key` is
  /// `None` the suggestions are for an array element.
  fn value_options(&self, path: &[PathSeg], key: Option<&str>) -> Vec<ValueOption> {
    self.schema_set_for_path(path).value_options_for(key)
  }
}

/// One or more schema nodes describing the same object or array. Property and
/// value suggestions are the union across all of them, with the first node
/// taking precedence on conflicts.
struct SchemaSet<'a> {
  refs: Vec<SchemaRef<'a>>,
}

impl<'a> SchemaSet<'a> {
  fn empty() -> Self {
    SchemaSet { refs: Vec::new() }
  }

  fn single(node: SchemaRef<'a>) -> Self {
    SchemaSet { refs: vec![node] }
  }

  fn property_names(&self) -> Vec<(String, SchemaRef<'a>)> {
    let mut result: Vec<(String, SchemaRef<'a>)> = Vec::new();
    for node in &self.refs {
      for (name, prop) in node.property_names() {
        if !result.iter().any(|(existing, _)| existing == &name) {
          result.push((name, prop));
        }
      }
    }
    result
  }

  fn property(&self, key: &str) -> Option<SchemaRef<'a>> {
    self.refs.iter().find_map(|node| node.property(key))
  }

  fn item(&self) -> Option<SchemaRef<'a>> {
    self.refs.iter().find_map(|node| node.item())
  }

  fn value_options_for(&self, key: Option<&str>) -> Vec<ValueOption> {
    let mut result: Vec<ValueOption> = Vec::new();
    for node in &self.refs {
      let target = match key {
        Some(key) => node.property(key),
        None => node.item(),
      };
      if let Some(target) = target {
        for option in target.value_options() {
          if !result.iter().any(|existing| existing.insert_text == option.insert_text) {
            result.push(option);
          }
        }
      }
    }
    result
  }
}

fn navigate<'a>(schema: SchemaRef<'a>, seg: &PathSeg) -> Option<SchemaRef<'a>> {
  match seg {
    PathSeg::Key(key) => schema.property(key),
    PathSeg::Elem => schema.item(),
  }
}

fn navigate_set<'a>(set: SchemaSet<'a>, segs: &[PathSeg]) -> SchemaSet<'a> {
  let mut refs = Vec::new();
  for node in set.refs {
    let mut current = Some(node);
    for seg in segs {
      current = current.and_then(|node| navigate(node, seg));
    }
    refs.extend(current);
  }
  SchemaSet { refs }
}

/// Digs the override-entry object schema out of a plugin section's `overrides`
/// property, which is an `anyOf` of a single object or an array of them.
fn override_item(schema: SchemaRef<'_>) -> Option<SchemaRef<'_>> {
  let schema = schema.deref();
  if schema.node.get("properties").is_some() {
    return Some(schema);
  }
  if let Some(item) = schema.item()
    && item.deref().node.get("properties").is_some()
  {
    return Some(item);
  }
  for keyword in ["anyOf", "oneOf", "allOf"] {
    if let Some(Value::Array(branches)) = schema.node.get(keyword) {
      for branch in branches {
        if let Some(found) = override_item(SchemaRef { doc: schema.doc, node: branch }) {
          return Some(found);
        }
      }
    }
  }
  None
}

/// A reference into a JSON schema document. `doc` is the root used for `$ref`
/// resolution; `node` is the current schema object.
#[derive(Clone, Copy)]
struct SchemaRef<'a> {
  doc: &'a Value,
  node: &'a Value,
}

impl<'a> SchemaRef<'a> {
  /// Follows any `$ref` (a `#/...` JSON pointer within the same document).
  fn deref(self) -> SchemaRef<'a> {
    let mut node = self.node;
    for _ in 0..10 {
      let Some(Value::String(reference)) = node.get("$ref") else {
        break;
      };
      match resolve_pointer(self.doc, reference) {
        Some(target) => node = target,
        None => break,
      }
    }
    SchemaRef { doc: self.doc, node }
  }

  fn property(self, key: &str) -> Option<SchemaRef<'a>> {
    let me = self.deref();
    if let Some(prop) = me.node.get("properties").and_then(|p| p.get(key)) {
      return Some(SchemaRef { doc: self.doc, node: prop });
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
      if let Some(Value::Array(branches)) = me.node.get(keyword) {
        for branch in branches {
          if let Some(found) = (SchemaRef { doc: self.doc, node: branch }).property(key) {
            return Some(found);
          }
        }
      }
    }
    match me.node.get("additionalProperties") {
      Some(node @ Value::Object(_)) => Some(SchemaRef { doc: self.doc, node }),
      _ => None,
    }
  }

  fn item(self) -> Option<SchemaRef<'a>> {
    let me = self.deref();
    match me.node.get("items") {
      // only single-schema arrays are handled (not tuple validation)
      Some(node @ (Value::Object(_) | Value::Bool(_))) => Some(SchemaRef { doc: self.doc, node }),
      _ => {
        for keyword in ["allOf", "anyOf", "oneOf"] {
          if let Some(Value::Array(branches)) = me.node.get(keyword) {
            for branch in branches {
              if let Some(found) = (SchemaRef { doc: self.doc, node: branch }).item() {
                return Some(found);
              }
            }
          }
        }
        None
      }
    }
  }

  fn property_names(self) -> Vec<(String, SchemaRef<'a>)> {
    let me = self.deref();
    let mut result = Vec::new();
    if let Some(Value::Object(props)) = me.node.get("properties") {
      for (name, node) in props {
        result.push((name.clone(), SchemaRef { doc: self.doc, node }));
      }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
      if let Some(Value::Array(branches)) = me.node.get(keyword) {
        for branch in branches {
          result.extend((SchemaRef { doc: self.doc, node: branch }).property_names());
        }
      }
    }
    result
  }

  fn value_options(self) -> Vec<ValueOption> {
    let me = self.deref();
    let mut options: Vec<ValueOption> = Vec::new();
    let push = |options: &mut Vec<ValueOption>, value: &Value, documentation: Option<String>| {
      if let Some(option) = ValueOption::from_json(value, documentation)
        && !options.iter().any(|o| o.insert_text == option.insert_text)
      {
        options.push(option);
      }
    };

    if let Some(Value::Array(values)) = me.node.get("enum") {
      for value in values {
        push(&mut options, value, None);
      }
    }
    if let Some(value) = me.node.get("const") {
      push(&mut options, value, me.description().map(str::to_string));
    }
    for keyword in ["oneOf", "anyOf"] {
      if let Some(Value::Array(branches)) = me.node.get(keyword) {
        for branch in branches {
          let branch_ref = (SchemaRef { doc: self.doc, node: branch }).deref();
          if let Some(value) = branch_ref.node.get("const") {
            push(&mut options, value, branch_ref.description().map(str::to_string));
          } else if let Some(Value::Array(values)) = branch_ref.node.get("enum") {
            for value in values {
              push(&mut options, value, None);
            }
          }
        }
      }
    }
    if options.is_empty() && me.has_type("boolean") {
      push(&mut options, &Value::Bool(true), None);
      push(&mut options, &Value::Bool(false), None);
    }

    options
  }

  fn description(self) -> Option<&'a str> {
    self.node.get("description").and_then(|d| d.as_str())
  }

  fn has_type(self, name: &str) -> bool {
    match self.node.get("type") {
      Some(Value::String(s)) => s == name,
      Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(name)),
      _ => false,
    }
  }

  fn type_label(self) -> Option<String> {
    match self.node.get("type") {
      Some(Value::String(s)) => Some(s.clone()),
      Some(Value::Array(arr)) => {
        let joined = arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" | ");
        (!joined.is_empty()).then_some(joined)
      }
      _ => {
        if self.node.get("enum").is_some() || self.node.get("oneOf").is_some() || self.node.get("anyOf").is_some() {
          Some("enum".to_string())
        } else {
          None
        }
      }
    }
  }
}

fn resolve_pointer<'a>(doc: &'a Value, reference: &str) -> Option<&'a Value> {
  let pointer = reference.strip_prefix('#')?;
  if pointer.is_empty() {
    return Some(doc);
  }
  let mut current = doc;
  for part in pointer.split('/').skip(1) {
    let part = part.replace("~1", "/").replace("~0", "~");
    current = current.get(&part)?;
  }
  Some(current)
}

struct NameOption {
  name: String,
  detail: Option<String>,
  documentation: Option<String>,
}

struct ValueOption {
  /// The text inserted into the document (valid JSON, ex. `"auto"` or `true`).
  insert_text: String,
  /// The label/filter text shown to the user (ex. `auto`).
  display: String,
  documentation: Option<String>,
}

impl ValueOption {
  fn from_json(value: &Value, documentation: Option<String>) -> Option<Self> {
    let insert_text = serde_json::to_string(value).ok()?;
    let display = match value {
      Value::String(s) => s.clone(),
      _ => insert_text.clone(),
    };
    Some(ValueOption {
      insert_text,
      display,
      documentation,
    })
  }
}

// === Pure analysis ===

#[derive(Debug, Clone, PartialEq)]
enum PathSeg {
  Key(String),
  Elem,
}

/// A scanned token with its byte range and whether it can be "edited" (ie. the
/// cursor being inside it means the user is typing that value).
struct Tok {
  kind: TokKind,
  start: usize,
  end: usize,
}

#[derive(Clone)]
enum TokKind {
  OpenBrace,
  CloseBrace,
  OpenBracket,
  CloseBracket,
  Comma,
  Colon,
  /// A string literal, with its decoded contents.
  Str(String),
  /// A bare word (loose property name or partially typed value).
  Word(String),
  /// A scalar that can't be a key in valid usage (boolean/number/null).
  Scalar(String),
  Comment {
    /// Whether text typed at the end of the comment is part of it, which is
    /// the case for a line comment and a block comment that isn't closed.
    open_ended: bool,
  },
}

impl Tok {
  fn is_editable(&self) -> bool {
    matches!(self.kind, TokKind::Str(_) | TokKind::Word(_) | TokKind::Scalar(_))
  }

  fn is_string(&self) -> bool {
    matches!(self.kind, TokKind::Str(_))
  }

  /// Gets whether this is a comment that text typed at `offset` would be in.
  fn is_comment_containing(&self, offset: usize) -> bool {
    match self.kind {
      TokKind::Comment { open_ended } => self.start < offset && (offset < self.end || open_ended && offset == self.end),
      _ => false,
    }
  }

  fn scalar_text(&self) -> Option<&str> {
    match &self.kind {
      TokKind::Str(s) | TokKind::Word(s) | TokKind::Scalar(s) => Some(s),
      _ => None,
    }
  }
}

/// Scans the whole text into tokens. A config file being edited is rarely
/// valid, so what the scanner rejects is recovered from instead of being the
/// end of the tokens. `cursor` is the offset being completed or hovered.
fn scan_tokens(text: &str, cursor: usize) -> Vec<Tok> {
  let mut tokens = Vec::new();
  // the scanner can't continue after an error, so a new one is started on
  // the text after each recovered token
  let mut base = 0;
  while base < text.len() {
    let mut scanner = Scanner::new(&text[base..], &Default::default());
    loop {
      let result = scanner.scan();
      let start = base + scanner.token_start();
      let end = base + scanner.token_end();
      let kind = match result {
        Ok(Some(token)) => match token {
          Token::OpenBrace => Some(TokKind::OpenBrace),
          Token::CloseBrace => Some(TokKind::CloseBrace),
          Token::OpenBracket => Some(TokKind::OpenBracket),
          Token::CloseBracket => Some(TokKind::CloseBracket),
          Token::Comma => Some(TokKind::Comma),
          Token::Colon => Some(TokKind::Colon),
          // the scanner's strings run across lines to the next quote, which
          // for a string that isn't closed yet is the start of another string
          Token::String(_) if text[start..end].contains('\n') => None,
          Token::String(value) => Some(TokKind::Str(value.into_owned())),
          Token::Word(value) => Some(TokKind::Word(value.to_string())),
          Token::Boolean(value) => Some(TokKind::Scalar(value.to_string())),
          Token::Number(value) => Some(TokKind::Scalar(value.to_string())),
          Token::Null => Some(TokKind::Scalar("null".to_string())),
          Token::CommentLine(_) => Some(TokKind::Comment { open_ended: true }),
          Token::CommentBlock(_) => Some(TokKind::Comment { open_ended: false }),
        },
        Ok(None) => return tokens,
        Err(_) => None,
      };
      match kind {
        Some(kind) => tokens.push(Tok { kind, start, end }),
        None => {
          let (kind, end) = recover_token(text, start, cursor);
          tokens.extend(kind.map(|kind| Tok { kind, start, end }));
          base = end;
          break;
        }
      }
    }
  }
  tokens
}

/// Gets the token (if any) and its end for the text at `start` that the
/// scanner didn't accept. The end is always after the start.
fn recover_token(text: &str, start: usize, cursor: usize) -> (Option<TokKind>, usize) {
  let rest = &text[start..];
  if rest.starts_with(['"', '\'']) {
    let (value, end) = recover_string(text, start, cursor);
    (Some(TokKind::Str(value)), end)
  } else if rest.starts_with("/*") {
    // a block comment that isn't closed
    (Some(TokKind::Comment { open_ended: true }), text.len())
  } else {
    // a word followed by punctuation (ex. `tr}`) or an incomplete number
    let len = word_len(rest);
    if len > 0 {
      (Some(TokKind::Word(rest[..len].to_string())), start + len)
    } else {
      // skip the character the scanner doesn't know
      (None, start + rest.chars().next().map(char::len_utf8).unwrap_or(1))
    }
  }
}

/// Gets the contents and end of the string starting with the quote at `start`
/// that either isn't closed on its line or has an invalid escape.
fn recover_string(text: &str, start: usize, cursor: usize) -> (String, usize) {
  let bytes = text.as_bytes();
  let quote = bytes[start];
  let content_start = start + 1;
  let mut index = content_start;
  // the bytes being looked for are never part of a multi-byte character
  let line_end = loop {
    match bytes.get(index) {
      None | Some(b'\n') => break index,
      Some(b'\r') if bytes.get(index + 1) == Some(&b'\n') => break index,
      Some(&b) if b == quote => return (text[content_start..index].to_string(), index + 1),
      // skip the escaped character, which might be a quote
      Some(b'\\') if !matches!(bytes.get(index + 1), None | Some(b'\r' | b'\n')) => index += 2,
      Some(_) => index += 1,
    }
  };

  // The string isn't closed. Anything could follow where it's being typed
  // (ex. `{ "lin| }`), so there it ends at the cursor or at the end of the
  // word the cursor is in the middle of.
  let end = if start < cursor && cursor <= line_end && text.is_char_boundary(cursor) {
    cursor + word_len(&text[cursor..line_end])
  } else {
    line_end
  };
  (text[content_start..end].to_string(), end)
}

/// Gets the length in bytes of the bare word at the start of the text.
fn word_len(text: &str) -> usize {
  text
    .find(|c: char| !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.' | '+'))
    .unwrap_or(text.len())
}

#[derive(Debug)]
enum Frame {
  Object {
    key_in_parent: Option<PathSeg>,
    last_key: Option<String>,
    after_colon: bool,
    keys: Vec<String>,
  },
  Array {
    key_in_parent: Option<PathSeg>,
  },
}

enum Position {
  /// Completing/hovering a property name.
  ObjectKey,
  /// Completing/hovering the value of `key`.
  ObjectValue { key: String },
  /// Completing/hovering an array element value.
  ArrayValue,
}

struct Analysis {
  /// Path to the innermost container the cursor is in.
  container_path: Vec<PathSeg>,
  position: Position,
  /// Keys already present in the innermost object, other than the one the
  /// cursor is in.
  existing_keys: Vec<String>,
  /// The byte range that an accepted completion should replace.
  replace_range: (usize, usize),
  /// Whether the cursor sits inside a quoted string.
  in_string: bool,
}

fn analyze(tokens: &[Tok], offset: usize) -> Option<Analysis> {
  // nothing in a comment is a property name or value
  if tokens.iter().any(|t| t.is_comment_containing(offset)) {
    return None;
  }

  // the token the cursor is editing (cursor within an editable token)
  let edit_idx = tokens.iter().position(|t| t.is_editable() && t.start <= offset && offset <= t.end);
  let boundary = edit_idx.unwrap_or_else(|| tokens.iter().position(|t| t.end > offset).unwrap_or(tokens.len()));

  let mut stack: Vec<Frame> = Vec::new();
  for tok in &tokens[..boundary] {
    apply_token(&mut stack, tok);
  }

  let container_path = container_path(&stack);
  let position = match stack.last() {
    Some(Frame::Object { last_key, after_colon, .. }) => match (after_colon, last_key) {
      (true, Some(key)) => Position::ObjectValue { key: key.clone() },
      _ => Position::ObjectKey,
    },
    Some(Frame::Array { .. }) => Position::ArrayValue,
    // not inside any container (ex. empty document)
    None => return None,
  };

  // The keys after the cursor are also already present, so keep going to
  // the end of the container the cursor is in. That end is only known when
  // every container is closed by the end of the file. When one isn't (ex. the
  // closing brace of the object being typed doesn't exist yet), the next
  // closing brace might be the parent's and the keys before it the parent's,
  // so only the keys before the cursor are used.
  let container_depth = stack.len();
  let keys_before_cursor = object_keys(&stack[container_depth - 1]);
  let mut keys_at_close = None;
  let after_idx = if edit_idx.is_some() { boundary + 1 } else { boundary };
  for tok in &tokens[after_idx..] {
    let is_close = matches!(tok.kind, TokKind::CloseBrace | TokKind::CloseBracket);
    if is_close && keys_at_close.is_none() && stack.len() == container_depth {
      keys_at_close = Some(object_keys(&stack[container_depth - 1]));
    }
    apply_token(&mut stack, tok);
  }
  let existing_keys = match keys_at_close {
    Some(keys) if stack.is_empty() => keys,
    _ => keys_before_cursor,
  };

  let (replace_range, in_string) = match edit_idx {
    Some(idx) => ((tokens[idx].start, tokens[idx].end), tokens[idx].is_string()),
    None => ((offset, offset), false),
  };

  Some(Analysis {
    container_path,
    position,
    existing_keys,
    replace_range,
    in_string,
  })
}

/// Updates the stack of containers that are open for the next token.
fn apply_token(stack: &mut Vec<Frame>, tok: &Tok) {
  match &tok.kind {
    TokKind::OpenBrace => {
      let key_in_parent = key_in_parent(stack);
      stack.push(Frame::Object {
        key_in_parent,
        last_key: None,
        after_colon: false,
        keys: Vec::new(),
      });
    }
    TokKind::OpenBracket => {
      let key_in_parent = key_in_parent(stack);
      stack.push(Frame::Array { key_in_parent });
    }
    TokKind::CloseBrace | TokKind::CloseBracket => {
      stack.pop();
      // the closed container was a completed value of its parent property
      if let Some(Frame::Object { last_key, after_colon, .. }) = stack.last_mut() {
        *last_key = None;
        *after_colon = false;
      }
    }
    TokKind::Colon => {
      if let Some(Frame::Object { after_colon, .. }) = stack.last_mut() {
        *after_colon = true;
      }
    }
    TokKind::Comma => {
      if let Some(Frame::Object { last_key, after_colon, .. }) = stack.last_mut() {
        *last_key = None;
        *after_colon = false;
      }
    }
    TokKind::Comment { .. } => {}
    TokKind::Str(_) | TokKind::Word(_) | TokKind::Scalar(_) => {
      if let Some(Frame::Object {
        last_key, after_colon, keys, ..
      }) = stack.last_mut()
      {
        if *after_colon {
          // a scalar value completes the property
          *last_key = None;
          *after_colon = false;
        } else {
          let text = tok.scalar_text().unwrap_or("").to_string();
          keys.push(text.clone());
          *last_key = Some(text);
        }
      }
    }
  }
}

/// Gets the keys seen so far in the container, which an array has none of.
fn object_keys(frame: &Frame) -> Vec<String> {
  match frame {
    Frame::Object { keys, .. } => keys.clone(),
    Frame::Array { .. } => Vec::new(),
  }
}

fn key_in_parent(stack: &[Frame]) -> Option<PathSeg> {
  match stack.last() {
    Some(Frame::Object { last_key, .. }) => last_key.clone().map(PathSeg::Key),
    Some(Frame::Array { .. }) => Some(PathSeg::Elem),
    None => None,
  }
}

fn container_path(stack: &[Frame]) -> Vec<PathSeg> {
  stack
    .iter()
    .filter_map(|frame| match frame {
      Frame::Object { key_in_parent, .. } => key_in_parent.clone(),
      Frame::Array { key_in_parent } => key_in_parent.clone(),
    })
    .collect()
}

fn completions_for(schema: &CompositeSchema, text: &str, line_index: &LineIndex, offset: usize) -> Vec<lsp::CompletionItem> {
  let tokens = scan_tokens(text, offset);
  let Some(analysis) = analyze(&tokens, offset) else {
    return Vec::new();
  };
  let range = lsp_range(line_index, analysis.replace_range.0, analysis.replace_range.1);

  match &analysis.position {
    Position::ObjectKey => {
      let options = schema.name_options(&analysis.container_path, &analysis.existing_keys);
      options
        .into_iter()
        .map(|option| {
          let new_text = format!("\"{}\"", option.name);
          let filter_text = if analysis.in_string { new_text.clone() } else { option.name.clone() };
          lsp::CompletionItem {
            label: option.name,
            kind: Some(lsp::CompletionItemKind::PROPERTY),
            detail: option.detail,
            documentation: option.documentation.map(markdown),
            filter_text: Some(filter_text),
            text_edit: Some(lsp::CompletionTextEdit::Edit(lsp::TextEdit { range, new_text })),
            ..Default::default()
          }
        })
        .collect()
    }
    Position::ObjectValue { key } => value_items(schema, &analysis, range, Some(key)),
    Position::ArrayValue => value_items(schema, &analysis, range, None),
  }
}

fn value_items(schema: &CompositeSchema, analysis: &Analysis, range: lsp::Range, key: Option<&str>) -> Vec<lsp::CompletionItem> {
  schema
    .value_options(&analysis.container_path, key)
    .into_iter()
    .map(|option| {
      let filter_text = if analysis.in_string {
        option.insert_text.clone()
      } else {
        option.display.clone()
      };
      lsp::CompletionItem {
        label: option.display,
        kind: Some(lsp::CompletionItemKind::VALUE),
        documentation: option.documentation.map(markdown),
        filter_text: Some(filter_text),
        text_edit: Some(lsp::CompletionTextEdit::Edit(lsp::TextEdit {
          range,
          new_text: option.insert_text,
        })),
        ..Default::default()
      }
    })
    .collect()
}

fn hover_for(schema: &CompositeSchema, text: &str, line_index: &LineIndex, offset: usize) -> Option<lsp::Hover> {
  let tokens = scan_tokens(text, offset);
  // find the token under the cursor (inclusive of its end so hovering the last
  // character still resolves)
  let idx = tokens.iter().position(|t| t.is_editable() && t.start <= offset && offset <= t.end)?;
  let analysis = analyze(&tokens, tokens[idx].start)?;

  let set = schema.schema_set_for_path(&analysis.container_path);
  let target = match &analysis.position {
    // the token is a property name
    Position::ObjectKey => {
      let key = tokens[idx].scalar_text()?;
      set.property(key)?
    }
    // the token is a value
    Position::ObjectValue { key } => set.property(key)?,
    Position::ArrayValue => set.item()?,
  };
  let target = target.deref();

  let mut markdown_text = String::new();
  if let Some(type_label) = target.type_label() {
    markdown_text.push_str(&format!("*{}*\n\n", type_label));
  }
  if let Some(description) = target.description() {
    markdown_text.push_str(description);
  }
  if markdown_text.trim().is_empty() {
    return None;
  }

  Some(lsp::Hover {
    contents: lsp::HoverContents::Markup(lsp::MarkupContent {
      kind: lsp::MarkupKind::Markdown,
      value: markdown_text,
    }),
    range: Some(lsp_range(line_index, tokens[idx].start, tokens[idx].end)),
  })
}

fn lsp_range(line_index: &LineIndex, start: usize, end: usize) -> lsp::Range {
  lsp::Range {
    start: line_index.position_utf16_from_utf8_offset(TextSize::from(start as u32)),
    end: line_index.position_utf16_from_utf8_offset(TextSize::from(end as u32)),
  }
}

fn markdown(value: String) -> lsp::Documentation {
  lsp::Documentation::MarkupContent(lsp::MarkupContent {
    kind: lsp::MarkupKind::Markdown,
    value,
  })
}

#[cfg(test)]
mod test {
  use dprint_core::async_runtime::future;

  use crate::environment::TestEnvironment;

  use super::*;

  const SCHEMA_URL: &str = "https://plugins.dprint.dev/test/schema.json";

  /// Splits a `%`-marked string into its text and the cursor's byte offset.
  fn at_cursor(text_with_marker: &str) -> (String, usize) {
    let offset = text_with_marker.find('%').expect("missing % cursor marker");
    (text_with_marker.replacen('%', "", 1), offset)
  }

  fn base_only() -> CompositeSchema {
    CompositeSchema {
      base: Rc::new(serde_json::from_str(DPRINT_CONFIG_SCHEMA).unwrap()),
      plugins: Vec::new(),
      is_missing_downloading_schema: false,
    }
  }

  fn with_typescript_plugin() -> CompositeSchema {
    let mut schema = base_only();
    schema.plugins.push(PluginSchema {
      config_key: "typescript".to_string(),
      name: "TypeScript".to_string(),
      schema: Some(Rc::new(serde_json::json!({
        "type": "object",
        "properties": {
          "semiColons": {
            "type": "string",
            "description": "How to use semi-colons.",
            "oneOf": [
              { "const": "always", "description": "Always uses semi-colons." },
              { "const": "asNeeded", "description": "Only when necessary." }
            ]
          },
          "lineWidth": { "type": "number", "description": "Plugin specific line width." }
        }
      }))),
    });
    schema
  }

  fn complete(schema: &CompositeSchema, text_with_marker: &str) -> Vec<lsp::CompletionItem> {
    let (text, offset) = at_cursor(text_with_marker);
    completions_for(schema, &text, &LineIndex::new(&text), offset)
  }

  fn labels(items: &[lsp::CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
  }

  fn labels_contain(items: &[lsp::CompletionItem], label: &str) -> bool {
    items.iter().any(|item| item.label == label)
  }

  fn new_text(item: &lsp::CompletionItem) -> &str {
    match item.text_edit.as_ref().unwrap() {
      lsp::CompletionTextEdit::Edit(edit) => &edit.new_text,
      _ => unreachable!(),
    }
  }

  fn item<'a>(items: &'a [lsp::CompletionItem], label: &str) -> &'a lsp::CompletionItem {
    items
      .iter()
      .find(|i| i.label == label)
      .unwrap_or_else(|| panic!("missing completion: {}", label))
  }

  fn edit(item: &lsp::CompletionItem) -> &lsp::TextEdit {
    match item.text_edit.as_ref().unwrap() {
      lsp::CompletionTextEdit::Edit(edit) => edit,
      _ => unreachable!(),
    }
  }

  /// Gets the text that results from accepting the completion with the
  /// provided label at the `%` cursor.
  fn accept(schema: &CompositeSchema, text_with_marker: &str, label: &str) -> String {
    let (text, offset) = at_cursor(text_with_marker);
    let line_index = LineIndex::new(&text);
    let items = completions_for(schema, &text, &line_index, offset);
    let edit = edit(item(&items, label));
    assert_eq!(edit.range.start.line, edit.range.end.line, "edit should be on a single line");
    let range = line_index.get_text_range(edit.range).unwrap();
    let mut text = text;
    text.replace_range(usize::from(range.start())..usize::from(range.end()), &edit.new_text);
    text
  }

  fn with_boolean_array_plugin() -> CompositeSchema {
    let mut schema = base_only();
    schema.plugins.push(PluginSchema {
      config_key: "test".to_string(),
      name: "Test".to_string(),
      schema: Some(Rc::new(serde_json::json!({
        "type": "object",
        "properties": {
          "flags": { "type": "array", "items": { "type": "boolean" } }
        }
      }))),
    });
    schema
  }

  #[test]
  fn registers_completion_and_hover_for_config_files() {
    fn get(capabilities: Value) -> ConfigFileCapabilities {
      get_config_file_capabilities(&serde_json::from_value(capabilities).unwrap())
    }

    let document_selector = serde_json::json!([
      { "pattern": "**/dprint.json" },
      { "pattern": "**/dprint.jsonc" },
      { "pattern": "**/.dprint.json" },
      { "pattern": "**/.dprint.jsonc" },
    ]);

    let result = get(serde_json::json!({
      "textDocument": {
        "completion": { "dynamicRegistration": true },
        "hover": { "dynamicRegistration": true },
      }
    }));
    assert_eq!(result.completion_provider, None);
    assert_eq!(result.hover_provider, None);
    assert_eq!(
      serde_json::to_value(&result.registrations).unwrap(),
      serde_json::json!([
        {
          "id": "dprint-config-textDocument/completion",
          "method": "textDocument/completion",
          "registerOptions": { "documentSelector": document_selector, "triggerCharacters": ["\"", ":"] },
        },
        {
          "id": "dprint-config-textDocument/hover",
          "method": "textDocument/hover",
          "registerOptions": { "documentSelector": document_selector },
        },
      ])
    );

    // only registers what the client supports registering
    let result = get(serde_json::json!({
      "textDocument": {
        "completion": { "dynamicRegistration": true },
        "hover": { "dynamicRegistration": false },
      }
    }));
    assert_eq!(result.completion_provider, None);
    assert_eq!(result.hover_provider, Some(lsp::HoverProviderCapability::Simple(true)));
    assert_eq!(
      result.registrations.iter().map(|r| r.method.as_str()).collect::<Vec<_>>(),
      vec!["textDocument/completion"]
    );

    // provides them for every document when they can't be registered
    for capabilities in [serde_json::json!({}), serde_json::json!({ "textDocument": { "completion": {}, "hover": {} } })] {
      let result = get(capabilities);
      assert_eq!(
        serde_json::to_value(&result.completion_provider).unwrap(),
        serde_json::json!({ "triggerCharacters": ["\"", ":"] })
      );
      assert_eq!(result.hover_provider, Some(lsp::HoverProviderCapability::Simple(true)));
      assert!(result.registrations.is_empty());
    }
  }

  #[test]
  fn downloads_plugin_schema_once() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(SCHEMA_URL, br#"{ "title": "first" }"#);
    environment.clone().run_in_runtime(async move {
      let schemas = PluginSchemas::new(environment.clone(), SCHEMA_DOWNLOAD_WAIT);
      // requests at the same time, one with two plugins that have the same
      // url and one with whitespace around the url
      let padded_url = format!(" {} ", SCHEMA_URL);
      let (first, second) = future::join(schemas.get_all(&[SCHEMA_URL, SCHEMA_URL, ""]), schemas.get_all(&[&padded_url])).await;
      assert_eq!(first.len(), 3);
      assert_eq!(first[0].as_ref().unwrap()["title"], "first");
      assert_eq!(first[1].as_ref().unwrap()["title"], "first");
      assert!(first[2].is_none());
      assert_eq!(second[0].as_ref().unwrap()["title"], "first");
      assert_eq!(environment.remote_file_request_count(SCHEMA_URL), 1);

      // and a later request
      environment.add_remote_file(SCHEMA_URL, br#"{ "title": "second" }"#);
      let third = schemas.get_all(&[SCHEMA_URL]).await;
      assert_eq!(third[0].as_ref().unwrap()["title"], "first");
      assert_eq!(environment.remote_file_request_count(SCHEMA_URL), 1);
    });
  }

  #[test]
  fn retries_plugin_schema_that_failed_after_interval() {
    let environment = TestEnvironment::new();
    environment.set_fs_time(1_000);
    environment.add_remote_file(SCHEMA_URL, b"<html>Bad Gateway</html>");
    environment.add_remote_file_error("https://plugins.dprint.dev/error.json", "Offline");
    environment.clone().run_in_runtime(async move {
      let urls = [
        SCHEMA_URL,
        "https://plugins.dprint.dev/error.json",
        "https://plugins.dprint.dev/missing.json",
        "not a url",
      ];
      let schemas = PluginSchemas::new(environment.clone(), SCHEMA_DOWNLOAD_WAIT);
      assert!(schemas.get_all(&urls).await.iter().all(|s| s.is_none()));
      assert_eq!(environment.remote_file_request_count(urls[0]), 1);
      assert_eq!(environment.remote_file_request_count(urls[1]), 1);
      assert_eq!(environment.remote_file_request_count(urls[2]), 1);

      // not before the interval is over
      for url in &urls[..3] {
        environment.add_remote_file(url, br#"{ "type": "object" }"#);
      }
      environment.set_fs_time(1_000 + SCHEMA_RETRY_INTERVAL.as_secs() - 1);
      assert!(schemas.get_all(&urls).await.iter().all(|s| s.is_none()));
      assert_eq!(environment.remote_file_request_count(urls[0]), 1);

      environment.set_fs_time(1_000 + SCHEMA_RETRY_INTERVAL.as_secs());
      let result = schemas.get_all(&urls).await;
      assert!(result[..3].iter().all(|s| s.is_some()));
      assert!(result[3].is_none());
      assert_eq!(environment.remote_file_request_count(urls[0]), 2);
      assert_eq!(environment.remote_file_request_count(urls[1]), 2);
      assert_eq!(environment.remote_file_request_count(urls[2]), 2);
    });
  }

  #[test]
  fn does_not_wait_long_on_slow_plugin_schema_download() {
    const SLOW_URL: &str = "https://example.com/slow.json";
    let environment = TestEnvironment::new();
    environment.add_remote_file(SCHEMA_URL, br#"{ "type": "object" }"#);
    environment.add_unresponsive_remote_file(SLOW_URL);
    environment.clone().run_in_runtime(async move {
      let mut schemas = PluginSchemas::new(environment.clone(), Duration::from_millis(50));
      // the test fails instead of hanging when the wait isn't limited
      let urls = [SLOW_URL, SCHEMA_URL];
      let result = tokio::time::timeout(Duration::from_secs(30), schemas.get_all(&urls)).await.unwrap();
      assert!(result[0].is_none());
      assert!(result[1].is_some());
      assert!(schemas.is_any_downloading(&urls));
      assert!(!schemas.is_any_downloading(&[SCHEMA_URL, ""]));

      // the download that's still going is not waited on a second time, which
      // would be for longer than the timeout here, and it's not started again
      schemas.wait = Duration::from_secs(3_600);
      let result = tokio::time::timeout(Duration::from_secs(30), schemas.get_all(&urls)).await.unwrap();
      assert!(result[0].is_none());
      assert!(result[1].is_some());
      assert_eq!(environment.remote_file_request_count(SLOW_URL), 1);
    });
  }

  #[test]
  fn gets_plugin_schema_of_download_that_was_given_up_on() {
    let environment = TestEnvironment::new();
    environment.add_remote_file(SCHEMA_URL, br#"{ "type": "object" }"#);
    environment.add_unresponsive_remote_file(SCHEMA_URL);
    environment.clone().run_in_runtime(async move {
      let schemas = PluginSchemas::new(environment.clone(), Duration::from_millis(50));
      assert!(schemas.get_all(&[SCHEMA_URL]).await[0].is_none());

      // the download wasn't stopped, so a request after it's done gets the
      // schema without another download
      environment.remove_unresponsive_remote_file(SCHEMA_URL);
      tokio::time::timeout(Duration::from_secs(30), async {
        while schemas.is_any_downloading(&[SCHEMA_URL]) {
          tokio::time::sleep(Duration::from_millis(10)).await;
        }
      })
      .await
      .unwrap();
      assert!(schemas.get_all(&[SCHEMA_URL]).await[0].is_some());
      assert_eq!(environment.remote_file_request_count(SCHEMA_URL), 1);
    });
  }

  #[test]
  fn embedded_schema_is_valid_json() {
    // ensures the include_str! path stays valid and the schema parses
    base_only();
  }

  #[test]
  fn completes_root_property_names() {
    let items = complete(&base_only(), "{\n  %\n}");
    let labels = labels(&items);
    assert!(labels.contains(&"lineWidth".to_string()));
    assert!(labels.contains(&"plugins".to_string()));
    assert!(labels.contains(&"newLineKind".to_string()));
    // a property name should be inserted quoted
    assert_eq!(new_text(item(&items, "lineWidth")), "\"lineWidth\"");
    assert_eq!(item(&items, "lineWidth").kind, Some(lsp::CompletionItemKind::PROPERTY));
  }

  #[test]
  fn completes_partial_property_name_inside_string() {
    let items = complete(&base_only(), "{ \"line%\" }");
    let line_width = item(&items, "lineWidth");
    // the whole string token (including quotes) is replaced
    assert_eq!(new_text(line_width), "\"lineWidth\"");
    assert_eq!(line_width.filter_text.as_deref(), Some("\"lineWidth\""));
  }

  #[test]
  fn excludes_already_present_keys() {
    let items = complete(&base_only(), "{ \"lineWidth\": 80, % }");
    let labels = labels(&items);
    assert!(!labels.contains(&"lineWidth".to_string()));
    assert!(labels.contains(&"indentWidth".to_string()));
  }

  #[test]
  fn completes_enum_values() {
    let items = complete(&base_only(), "{ \"newLineKind\": % }");
    let labels = labels(&items);
    assert_eq!(labels, vec!["auto", "crlf", "lf", "system"]);
    assert_eq!(new_text(item(&items, "auto")), "\"auto\"");
    assert_eq!(item(&items, "auto").kind, Some(lsp::CompletionItemKind::VALUE));
  }

  #[test]
  fn completes_enum_values_inside_string() {
    let items = complete(&base_only(), "{ \"newLineKind\": \"sys%\" }");
    let system = item(&items, "system");
    assert_eq!(new_text(system), "\"system\"");
    assert_eq!(system.filter_text.as_deref(), Some("\"system\""));
  }

  #[test]
  fn completes_boolean_values() {
    let items = complete(&base_only(), "{ \"useTabs\": % }");
    assert_eq!(labels(&items), vec!["true", "false"]);
    assert_eq!(new_text(item(&items, "true")), "true");
  }

  #[test]
  fn completes_plugin_config_key_at_root() {
    let items = complete(&with_typescript_plugin(), "{\n  %\n}");
    let typescript = item(&items, "typescript");
    assert_eq!(new_text(typescript), "\"typescript\"");
    assert_eq!(typescript.detail.as_deref(), Some("plugin"));
  }

  #[test]
  fn completes_plugin_section_property_names() {
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { % } }");
    let labels = labels(&items);
    // the plugin's own properties
    assert!(labels.contains(&"semiColons".to_string()));
    // plus the properties common to every plugin section
    assert!(labels.contains(&"locked".to_string()));
    assert!(labels.contains(&"associations".to_string()));
  }

  #[test]
  fn completes_plugin_nested_enum_values() {
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { \"semiColons\": % } }");
    assert_eq!(labels(&items), vec!["always", "asNeeded"]);
  }

  #[test]
  fn completes_inside_overrides_object() {
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { \"overrides\": { % } } }");
    let labels = labels(&items);
    // the override's own `files` key
    assert!(labels.contains(&"files".to_string()), "{:?}", labels);
    // plus the plugin's config that can be overridden
    assert!(labels.contains(&"semiColons".to_string()), "{:?}", labels);
  }

  #[test]
  fn completes_inside_overrides_array_element() {
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { \"overrides\": [{ % }] } }");
    let labels = labels(&items);
    assert!(labels.contains(&"files".to_string()), "{:?}", labels);
    assert!(labels.contains(&"semiColons".to_string()), "{:?}", labels);
  }

  #[test]
  fn completes_overridden_plugin_enum_values() {
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { \"overrides\": [{ \"semiColons\": % }] } }");
    assert_eq!(labels(&items), vec!["always", "asNeeded"]);
  }

  #[test]
  fn completes_overrides_without_plugin_schema() {
    // a plugin with no schema still offers the common override `files` key
    let mut schema = base_only();
    schema.plugins.push(PluginSchema {
      config_key: "exec".to_string(),
      name: "Exec".to_string(),
      schema: None,
    });
    let items = complete(&schema, "{ \"exec\": { \"overrides\": [{ % }] } }");
    assert!(labels(&items).contains(&"files".to_string()));
  }

  #[test]
  fn no_completions_outside_object() {
    assert!(complete(&base_only(), "%").is_empty());
  }

  #[test]
  fn completes_unterminated_string_before_another_property() {
    // the state right after typing a quote in an editor without auto-pairing,
    // where the next quote in the file opens the following property's name
    assert_eq!(
      accept(&base_only(), "{\n  \"new%\n  \"useTabs\": true\n}", "newLineKind"),
      "{\n  \"newLineKind\"\n  \"useTabs\": true\n}"
    );
    assert_eq!(
      accept(&base_only(), "{\r\n  \"new%\r\n  \"useTabs\": true\r\n}", "newLineKind"),
      "{\r\n  \"newLineKind\"\r\n  \"useTabs\": true\r\n}"
    );
    assert_eq!(
      accept(&base_only(), "{\n  \"newLineKind\": \"sys%\n  \"useTabs\": true\n}", "system"),
      "{\n  \"newLineKind\": \"system\"\n  \"useTabs\": true\n}"
    );
  }

  #[test]
  fn completes_unterminated_string_without_later_quote() {
    assert_eq!(accept(&base_only(), "{\n  \"lin%\n}", "lineWidth"), "{\n  \"lineWidth\"\n}");
    assert_eq!(accept(&base_only(), "{\n  \"%\n}", "lineWidth"), "{\n  \"lineWidth\"\n}");
    assert_eq!(accept(&base_only(), "{\n  \"lin%", "lineWidth"), "{\n  \"lineWidth\"");
    // only up to the cursor, which keeps what follows on the line
    assert_eq!(accept(&base_only(), "{ \"lin% }", "lineWidth"), "{ \"lineWidth\" }");
    assert_eq!(accept(&base_only(), "{ \"newLineKind\": \"sys% }", "system"), "{ \"newLineKind\": \"system\" }");
    // though the rest of a word the cursor is in the middle of is replaced
    assert_eq!(accept(&base_only(), "{\n  \"lin%eWid\n}", "lineWidth"), "{\n  \"lineWidth\"\n}");
    let items = complete(&base_only(), "{\n  \"lin%\n}");
    assert_eq!(item(&items, "lineWidth").filter_text.as_deref(), Some("\"lineWidth\""));
  }

  #[test]
  fn completes_after_non_ascii_text() {
    // é is two utf-8 bytes and one utf-16 code unit, 🦕 is four and two
    assert_eq!(
      accept(&base_only(), "{\n  // é🦕\n  \"line%\"\n}", "lineWidth"),
      "{\n  // é🦕\n  \"lineWidth\"\n}"
    );
    let items = complete(&base_only(), "{ /* é🦕 */ \"line%\" }");
    assert_eq!(
      edit(item(&items, "lineWidth")).range,
      lsp::Range {
        start: lsp::Position { line: 0, character: 12 },
        end: lsp::Position { line: 0, character: 18 },
      }
    );
  }

  #[test]
  fn completes_partial_bare_word_before_punctuation() {
    assert_eq!(accept(&base_only(), "{\"useTabs\": tr%}", "true"), "{\"useTabs\": true}");
    assert_eq!(
      accept(&base_only(), "{\n  \"useTabs\": fa%,\n  \"lineWidth\": 80\n}", "false"),
      "{\n  \"useTabs\": false,\n  \"lineWidth\": 80\n}"
    );
    assert_eq!(
      accept(&with_boolean_array_plugin(), "{ \"test\": { \"flags\": [true, fa%] } }", "false"),
      "{ \"test\": { \"flags\": [true, false] } }"
    );
    // a bare property name
    assert_eq!(accept(&base_only(), "{line%}", "lineWidth"), "{\"lineWidth\"}");
    let items = complete(&base_only(), "{\"useTabs\": tr%}");
    assert_eq!(item(&items, "true").filter_text.as_deref(), Some("true"));
    // what follows an incomplete word is still understood
    let items = complete(&base_only(), "{ \"useTabs\": tr, % }");
    assert!(labels_contain(&items, "lineWidth"));
    assert!(!labels_contain(&items, "useTabs"));
  }

  #[test]
  fn completes_after_string_with_invalid_escape() {
    // the scanner rejects `\q`, and the strings end with an escaped backslash
    // and have an escaped quote
    assert_eq!(accept(&base_only(), r#"{ "a\q\\": 1, % }"#, "lineWidth"), r#"{ "a\q\\": 1, "lineWidth" }"#);
    assert_eq!(
      accept(&base_only(), r#"{ "a\q\"": 1, "useTabs": % }"#, "true"),
      r#"{ "a\q\"": 1, "useTabs": true }"#
    );
  }

  #[test]
  fn excludes_keys_after_the_cursor() {
    let items = complete(&base_only(), "{\n  %\n  \"lineWidth\": 80\n}");
    assert!(!labels_contain(&items, "lineWidth"));
    assert!(labels_contain(&items, "indentWidth"));

    // including when the property being typed is incomplete
    let items = complete(&base_only(), "{\n  \"useTabs\": true,\n  \"%\n  \"lineWidth\": 80,\n  \"indentWidth\": 2\n}");
    assert!(!labels_contain(&items, "useTabs"));
    assert!(!labels_contain(&items, "lineWidth"));
    assert!(!labels_contain(&items, "indentWidth"));
    assert!(labels_contain(&items, "newLineKind"));

    // the property the cursor is in is still offered
    let items = complete(&base_only(), "{ \"line%Width\": 80 }");
    assert!(labels_contain(&items, "lineWidth"));
  }

  #[test]
  fn only_excludes_keys_of_the_object_the_cursor_is_in() {
    // keys of a nested object after the cursor
    let items = complete(&with_typescript_plugin(), "{\n  %\n  \"typescript\": { \"lineWidth\": 80 }\n}");
    assert!(labels_contain(&items, "lineWidth"));
    assert!(!labels_contain(&items, "typescript"));
    // keys of the parent object after the cursor
    let items = complete(&with_typescript_plugin(), "{ \"typescript\": { % }, \"lineWidth\": 80 }");
    assert!(labels_contain(&items, "lineWidth"));
  }

  #[test]
  fn keeps_parent_keys_after_the_cursor_in_unclosed_object() {
    // the closing brace is the root's, so the keys after the cursor aren't
    // the plugin object's
    let schema = with_typescript_plugin();
    for text in [
      "{\n  \"typescript\": {\n    \"%\n  \"lineWidth\": 120,\n  \"indentWidth\": 2\n}",
      "{\n  \"typescript\": {\n    %\n  \"lineWidth\": 120,\n  \"indentWidth\": 2\n}",
    ] {
      let items = complete(&schema, text);
      assert!(labels_contain(&items, "lineWidth"), "{}", text);
      assert!(labels_contain(&items, "semiColons"), "{}", text);
    }
    // the keys before the cursor are still excluded
    let items = complete(
      &schema,
      "{\n  \"typescript\": {\n    \"semiColons\": \"always\",\n    %\n  \"lineWidth\": 120\n}",
    );
    assert!(labels_contain(&items, "lineWidth"));
    assert!(!labels_contain(&items, "semiColons"));
    let items = complete(&base_only(), "{\n  \"useTabs\": true,\n  %\n  \"lineWidth\": 80\n");
    assert!(labels_contain(&items, "lineWidth"));
    assert!(!labels_contain(&items, "useTabs"));
  }

  #[test]
  fn skips_unknown_character() {
    let items = complete(&base_only(), "{ * \"useTabs\": true, % }");
    assert!(!labels_contain(&items, "useTabs"));
    assert!(labels_contain(&items, "lineWidth"));
    // also after the cursor, and when it's more than one byte
    let items = complete(&base_only(), "{ % * \"useTabs\": true, § \"indentWidth\": 2 }");
    assert!(!labels_contain(&items, "useTabs"));
    assert!(!labels_contain(&items, "indentWidth"));
    assert!(labels_contain(&items, "lineWidth"));
  }

  #[test]
  fn no_completions_inside_comment() {
    assert!(complete(&base_only(), "{\n  // see: %\n}").is_empty());
    assert!(complete(&base_only(), "{\n  // see%: more\n}").is_empty());
    assert!(complete(&base_only(), "{\n  \"useTabs\": // see: %\n}").is_empty());
    assert!(complete(&base_only(), "{ /* see: % */ }").is_empty());
    assert!(complete(&base_only(), "{\n  /* see: %\n}").is_empty());
    // not in the comment
    assert!(!complete(&base_only(), "{ /* see: */% }").is_empty());
    assert!(!complete(&base_only(), "{ %/* see: */ }").is_empty());
    assert!(!complete(&base_only(), "{\n  // see\n  %\n}").is_empty());
    assert!(!complete(&base_only(), "{\n  %// see\n}").is_empty());
  }

  #[test]
  fn hover_range_after_non_ascii_text() {
    let (text, offset) = at_cursor("{\n  // é🦕\n  /* é */ \"newLine%Kind\": \"auto\"\n}");
    let hover = hover_for(&base_only(), &text, &LineIndex::new(&text), offset).unwrap();
    assert_eq!(
      hover.range,
      Some(lsp::Range {
        start: lsp::Position { line: 2, character: 10 },
        end: lsp::Position { line: 2, character: 23 },
      })
    );
  }

  #[test]
  fn hovers_property_name() {
    let (text, offset) = at_cursor("{ \"newLine%Kind\": \"auto\" }");
    let hover = hover_for(&base_only(), &text, &LineIndex::new(&text), offset).unwrap();
    let lsp::HoverContents::Markup(content) = hover.contents else {
      unreachable!()
    };
    assert!(content.value.contains("The kind of newline to use."));
  }

  #[test]
  fn hovers_plugin_property_value() {
    let (text, offset) = at_cursor("{ \"typescript\": { \"semiColons\": \"alw%ays\" } }");
    let hover = hover_for(&with_typescript_plugin(), &text, &LineIndex::new(&text), offset).unwrap();
    let lsp::HoverContents::Markup(content) = hover.contents else {
      unreachable!()
    };
    assert!(content.value.contains("How to use semi-colons."));
  }
}
