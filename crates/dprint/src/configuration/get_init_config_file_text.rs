use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;
use anyhow::bail;
use dprint_core::async_runtime::FutureExt;
use dprint_core::async_runtime::future;
use dprint_core::plugins::wasm::{self};
use jsonc_parser::cst::CstInputValue;
use jsonc_parser::cst::CstRootNode;

use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::plugins::InfoFile;
use crate::plugins::InfoFilePluginInfo;
use crate::plugins::MinimumDependencyAgeError;
use crate::plugins::ResolveNpmLatestOptions;
use crate::plugins::read_info_file;
use crate::plugins::resolve_dependency_age_cutoff;
use crate::utils::DependencyAgeCutoff;
use crate::utils::DirEntriesHint;
use crate::utils::GitIgnoreTree;
use crate::utils::GitIgnoreTreeOptions;
use crate::utils::MinimumDependencyAgeArg;
use crate::utils::MultiSelectAction;
use crate::utils::MultiSelectDetailPart;
use crate::utils::MultiSelectFeatures;
use crate::utils::MultiSelectItem;
use crate::utils::MultiSelectState;
use crate::utils::SettingsCheckbox;
use crate::utils::SettingsEditorState;
use crate::utils::SettingsOptions;
use crate::utils::resolve_global_gitignore_lines;

use super::InitPluginOptionsLoader;
use super::InitPreferences;
use super::PreferredPlugin;
use super::apply_config_to_settings_options;
use super::get_config_from_settings_options;
use super::read_init_preferences;
use super::write_init_preferences;

/// Maximum number of files to look at when scanning the current directory to
/// decide which plugins to pre-select. Keeps `dprint init` fast in large repos.
const MAX_SCAN_FILES: usize = 1000;
/// Upper bound on directories visited during the scan so a deep tree with few
/// files can't make the scan walk forever.
const MAX_SCAN_DIRS: usize = 1000;

#[derive(Default)]
pub struct GetInitConfigFileTextOptions {
  /// Skip the interactive plugin prompt and accept the plugins selected based
  /// on the files in the current directory.
  pub non_interactive: bool,
  /// Don't write an npm plugin version published more recently than this.
  pub minimum_dependency_age: Option<MinimumDependencyAgeArg>,
  /// Directory the config file will be written to. An .npmrc setting
  /// `min-release-age` is looked for here and in its ancestors, so the
  /// age that applies is the one nearest the config file rather than the
  /// process cwd (which differs for `--global` and `--config`).
  pub config_dir: Option<PathBuf>,
  /// Loads a plugin's options so they can be edited from the plugin prompt.
  /// Without this only the files a plugin formats can be changed there.
  pub options_loader: Option<Rc<dyn InitPluginOptionsLoader>>,
}

/// Options for [`get_init_plugins_to_add`].
pub struct GetInitPluginsToAddOptions {
  /// Names, as they appear in the info file, of the plugins the config file
  /// already has in the order it has them. They're shown in the prompt so
  /// plugins can be added in between them, but can't be selected.
  pub existing_plugin_names: Vec<String>,
  /// Don't add an npm plugin version published more recently than this.
  pub minimum_dependency_age: Option<MinimumDependencyAgeArg>,
  /// Directory of the config file being added to. An .npmrc setting
  /// `min-release-age` is looked for here and in its ancestors.
  pub config_dir: Option<PathBuf>,
  /// Loads a plugin's options so they can be edited from the plugin prompt.
  pub options_loader: Option<Rc<dyn InitPluginOptionsLoader>>,
}

/// The outcome of prompting for plugins to add to a config file that already
/// exists.
pub enum InitPluginsToAdd {
  /// The config file already has every plugin the info file knows about, so
  /// there was nothing to prompt for.
  AllPluginsConfigured,
  /// The plugins to add to the config file in the order the user had them,
  /// which is in between the plugins it already has. Empty when the user
  /// selected nothing.
  Plugins(Vec<InitPluginToAdd>),
}

/// A plugin to add to a config file that already exists.
#[derive(Debug, PartialEq)]
pub struct InitPluginToAdd {
  /// The text to add to the config file's `plugins` array.
  pub entry: String,
  /// Name of the plugin the config file already has that the user put this
  /// one ahead of. It goes after all of them without one.
  pub before: Option<String>,
  /// The plugin's config key and what the user chose for its config block,
  /// when they chose anything. Unlike a new config file, nothing is
  /// scaffolded for a plugin that's added to one that already exists.
  pub config: Option<(String, serde_json::Value)>,
}

pub async fn get_init_config_file_text(environment: &impl Environment, options: GetInitConfigFileTextOptions) -> Result<String> {
  let info = read_plugin_info_file(environment).await;

  let selected_plugins = if let Some(info) = info {
    let latest_plugins = info.latest_plugins;
    // pre-select the plugins that match files found in the current directory
    let mut project_files = scan_project_files(environment);
    // the config file about to be written is JSON, so it counts as a project
    // file even though it doesn't exist yet — a json plugin is always relevant
    project_files.extensions.insert("json".to_string());
    let preferences = read_init_preferences(environment);
    let mut choices = get_plugin_choices(&latest_plugins, &preferences);
    let preferred_order = get_preferred_order(&latest_plugins, &preferences);
    let defaults = compute_default_selections(&latest_plugins, &project_files, &[], &choices, &preferred_order);
    // show the pre-selected plugins at the top of the list
    let order = display_order(&defaults, &[], &preferred_order);
    let ranks = plugin_ranks(latest_plugins.len(), &preferred_order);

    // an .npmrc setting `min-release-age` is looked for where the config file
    // is going to be written
    let age_cutoff = resolve_dependency_age_cutoff(options.minimum_dependency_age.as_ref(), options.config_dir.as_deref(), environment);

    // the plugins go in the config file in the order they're shown in
    let selected_indexes = if options.non_interactive {
      order.iter().copied().filter(|&i| defaults[i]).collect::<Vec<_>>()
    } else {
      prompt_for_plugins(
        environment,
        PluginPrompt {
          message: "Select plugins:",
          plugins: &latest_plugins,
          project_files: &project_files,
          defaults: &defaults,
          already_configured: &[],
          order: &order,
          ranks: &ranks,
          options_loader: options.options_loader.as_deref(),
          age_cutoff: age_cutoff.as_ref(),
        },
        &mut choices,
        &preferences,
      )
      .await?
    };

    // resolve concurrently — a plugin distributed on npm costs a registry round trip
    let entries = future::join_all(
      selected_indexes
        .iter()
        .map(|&index| resolve_plugin_entry(&latest_plugins[index], age_cutoff.as_ref(), environment)),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    let selected_plugins = selected_indexes
      .into_iter()
      .zip(entries)
      .map(|(index, entry)| {
        let plugin = latest_plugins[index].clone();
        let config = build_plugin_config(&plugin, &project_files, &choices[index]);
        SelectedPlugin { plugin, config, entry }
      })
      .collect::<Vec<_>>();
    Some(selected_plugins)
  } else {
    None
  };

  let json_text = match selected_plugins {
    Some(selected_plugins) if !selected_plugins.is_empty() => render_config_file(&selected_plugins),
    // plugin info was available, but nothing was selected
    Some(_) => "{\n  \"excludes\": [],\n  \"plugins\": [\n    // specify plugin urls here\n  ]\n}\n".to_string(),
    // the plugin info couldn't be downloaded
    None => "{\n  \"excludes\": [\n    \"**/*-lock.json\"\n  ],\n  \"plugins\": [\n    // specify plugin urls here\n  ]\n}\n".to_string(),
  };

  Ok(json_text)
}

/// Prompts for plugins to add to a config file that already exists,
/// pre-selecting the ones that match files in the current directory the config
/// doesn't already have a plugin for. The plugins it does have are shown in
/// the prompt for context, but can't be selected.
pub async fn get_init_plugins_to_add(environment: &impl Environment, options: GetInitPluginsToAddOptions) -> Result<InitPluginsToAdd> {
  let Some(info) = read_plugin_info_file(environment).await else {
    bail!("Could not get the latest plugin info, so there's nothing to select from.");
  };
  let latest_plugins = info.latest_plugins;
  let already_configured = latest_plugins
    .iter()
    .map(|plugin| options.existing_plugin_names.contains(&plugin.name))
    .collect::<Vec<_>>();
  if already_configured.iter().all(|configured| *configured) {
    return Ok(InitPluginsToAdd::AllPluginsConfigured);
  }

  // pre-select the plugins matching files in the current directory, leaving out
  // the file types the config file's plugins already handle
  let mut project_files = scan_project_files(environment);
  // the config file being added to is JSON, so a json plugin is always
  // relevant (the scan doesn't see it for a global config outside the cwd)
  project_files.extensions.insert("json".to_string());
  let preferences = read_init_preferences(environment);
  let mut choices = get_plugin_choices(&latest_plugins, &preferences);
  // what was remembered is what to start a plugin out with, which says
  // nothing about how the config file has the ones it already has
  for (choices, _) in choices.iter_mut().zip(&already_configured).filter(|(_, configured)| **configured) {
    *choices = Default::default();
  }
  let preferred_order = get_preferred_order(&latest_plugins, &preferences);
  let defaults = compute_default_selections(&latest_plugins, &project_files, &already_configured, &choices, &preferred_order);
  // show the config file's plugins the way it has them, which is ahead of
  // what gets added, and then the pre-selected plugins
  let mut configured_order = Vec::new();
  for name in &options.existing_plugin_names {
    if let Some(index) = latest_plugins.iter().position(|plugin| plugin.name == *name)
      && !configured_order.contains(&index)
    {
      configured_order.push(index);
    }
  }
  let order = display_order(&defaults, &configured_order, &preferred_order);
  let ranks = plugin_ranks(latest_plugins.len(), &preferred_order);
  // an .npmrc setting `min-release-age` is looked for where the config file lives
  let age_cutoff = resolve_dependency_age_cutoff(options.minimum_dependency_age.as_ref(), options.config_dir.as_deref(), environment);
  let shown_indexes = prompt_for_plugins(
    environment,
    PluginPrompt {
      message: "Select plugins to add:",
      plugins: &latest_plugins,
      project_files: &project_files,
      defaults: &defaults,
      already_configured: &already_configured,
      order: &order,
      ranks: &ranks,
      options_loader: options.options_loader.as_deref(),
      age_cutoff: age_cutoff.as_ref(),
    },
    &mut choices,
    &preferences,
  )
  .await?;
  // they're added to the config file where they were left among its plugins
  let mut selected_indexes = Vec::new();
  let mut befores = Vec::new();
  for (position, &index) in shown_indexes.iter().enumerate() {
    if already_configured[index] {
      continue;
    }
    let next_configured_index = shown_indexes[position + 1..].iter().find(|&&index| already_configured[index]);
    selected_indexes.push(index);
    befores.push(next_configured_index.map(|&index| latest_plugins[index].name.clone()));
  }

  // resolve concurrently — a plugin distributed on npm costs a registry round trip
  let entries = future::join_all(
    selected_indexes
      .iter()
      .map(|&index| resolve_plugin_entry(&latest_plugins[index], age_cutoff.as_ref(), environment)),
  )
  .await
  .into_iter()
  .collect::<Result<Vec<_>>>()?;
  let plugins = selected_indexes
    .into_iter()
    .zip(entries)
    .zip(befores)
    .map(|((index, entry), before)| {
      let plugin = &latest_plugins[index];
      let mut config = serde_json::Map::new();
      apply_choices_to_config(&mut config, plugin, &choices[index]);
      InitPluginToAdd {
        entry,
        before,
        config: config_key(plugin)
          .filter(|_| !config.is_empty())
          .map(|config_key| (config_key.to_string(), serde_json::Value::Object(config))),
      }
    })
    .collect();
  Ok(InitPluginsToAdd::Plugins(plugins))
}

/// Prompts for the plugins the user prefers, which is what `dprint init`
/// remembers: the selected plugins in the order they're in, along with the
/// files and options of each. It's saved when the user finishes.
///
/// This has nothing to do with the directory it's run in or a config file, so
/// no file is highlighted and nothing is initialized. What's saved is what
/// `dprint init` starts out with from then on.
pub async fn prompt_for_init_preferences(environment: &impl Environment, options_loader: Option<Rc<dyn InitPluginOptionsLoader>>) -> Result<()> {
  let Some(info) = read_plugin_info_file(environment).await else {
    bail!("Could not get the latest plugin info, so there's nothing to select from.");
  };
  let plugins = info.latest_plugins;
  let preferences = read_init_preferences(environment);
  let no_project_files = ProjectFiles::default();
  let age_cutoff = resolve_dependency_age_cutoff(None, None, environment);
  let mut choices = get_plugin_choices(&plugins, &preferences);
  let preferred_order = get_preferred_order(&plugins, &preferences);
  // the preferred plugins in their order, which is the order that's saved
  let ranks = plugin_ranks(plugins.len(), &preferred_order);
  let mut indexes = (0..plugins.len()).collect::<Vec<_>>();
  indexes.sort_by_key(|&index| ranks[index]);
  let items = indexes
    .into_iter()
    .map(|index| {
      let plugin = &plugins[index];
      MultiSelectItem::new(plugin.display_name().to_string(), preferred_order.contains(&index))
        .with_detail(plugin_prompt_detail(plugin, &choices[index], &no_project_files, false))
        .with_id(index)
        .with_rank(ranks[index])
    })
    .collect::<Vec<_>>();
  let mut state = MultiSelectState::new("Select the plugins to prefer whenever running dprint init:", items);
  state.help = Some("space: toggle, \u{2192}: options, shift+\u{2191}/\u{2193}: move, type to filter, enter: save, esc: cancel".to_string());
  state.features = MultiSelectFeatures {
    can_move: true,
    can_open: true,
    selected_first: true,
  };

  loop {
    match environment.get_multi_selection(&mut state)? {
      MultiSelectAction::Done => break,
      MultiSelectAction::Open(item_index) => {
        let index = state.items[item_index].id;
        let plugin = &plugins[index];
        let title = format!("{} when preferred", plugin.display_name());
        let old_choices = choices[index].clone();
        let options_source = options_loader.as_deref().map(|loader| (loader, age_cutoff.as_ref()));
        edit_plugin_choices(environment, &title, plugin, &no_project_files, options_source, &mut choices[index]).await?;
        state.items[item_index].detail = plugin_prompt_detail(plugin, &choices[index], &no_project_files, false);
        // what was changed is only remembered for a preferred plugin
        if choices[index] != old_choices && !state.items[item_index].is_selected {
          state.set_item_selected(item_index, true);
        }
      }
    }
  }

  let new_preferences = get_updated_preferences(&preferences, &plugins, &choices, &state.selected_ids());
  if new_preferences == preferences {
    log_stdout_info!(environment, "\nThe preferred plugins are the same as they were.");
    return Ok(());
  }
  let file_path = write_init_preferences(environment, &new_preferences)?;
  log_stdout_info!(environment, "\nSaved the preferred plugins to {}", file_path.display());
  Ok(())
}

/// What the user chose for a plugin beyond selecting it: either for the config
/// file being written or for what `dprint init` starts the plugin out with
/// every time. The two are never the same value, so changing one leaves the
/// other alone.
#[derive(Debug, Default, Clone, PartialEq)]
struct PluginChoices {
  /// Lowercased file extensions the plugin matches that it shouldn't format.
  excluded_extensions: Vec<String>,
  /// File names the plugin matches that it shouldn't format.
  excluded_file_names: Vec<String>,
  /// Options to write to the plugin's config block.
  config: serde_json::Map<String, serde_json::Value>,
}

impl PluginChoices {
  fn excludes_extension(&self, extension: &str) -> bool {
    let extension = extension.to_lowercase();
    self.excluded_extensions.contains(&extension)
  }

  fn excludes_file_name(&self, file_name: &str) -> bool {
    self.excluded_file_names.iter().any(|name| name == file_name)
  }

  /// Whether the plugin's files or options were changed.
  fn is_customized(&self) -> bool {
    !self.excluded_extensions.is_empty() || !self.excluded_file_names.is_empty() || !self.config.is_empty()
  }
}

/// The choices to start each plugin of the info file out with, which is what
/// was remembered for the plugins the user prefers.
fn get_plugin_choices(plugins: &[InfoFilePluginInfo], preferences: &InitPreferences) -> Vec<PluginChoices> {
  plugins
    .iter()
    .map(
      |plugin| match preferences.preferred_plugins.iter().find(|preferred| preferred.name == plugin.name) {
        Some(preferred) => PluginChoices {
          excluded_extensions: preferred.excluded_file_extensions.iter().map(|ext| ext.to_lowercase()).collect(),
          excluded_file_names: preferred.excluded_file_names.clone(),
          config: preferred.config.clone(),
        },
        None => Default::default(),
      },
    )
    .collect()
}

/// Indexes of the plugins the user prefers, in the order they prefer them.
fn get_preferred_order(plugins: &[InfoFilePluginInfo], preferences: &InitPreferences) -> Vec<usize> {
  let mut order = Vec::new();
  for preferred in &preferences.preferred_plugins {
    if let Some(index) = plugins.iter().position(|plugin| plugin.name == preferred.name)
      && !order.contains(&index)
    {
      order.push(index);
    }
  }
  order
}

/// What to show in the plugin prompt.
struct PluginPrompt<'a> {
  message: &'a str,
  plugins: &'a [InfoFilePluginInfo],
  project_files: &'a ProjectFiles,
  /// The plugins that start out selected.
  defaults: &'a [bool],
  /// The plugins a config file already has (empty when there's no config
  /// file yet). They're shown, but can't be selected.
  already_configured: &'a [bool],
  /// Indexes of the plugins in the order to show them in.
  order: &'a [usize],
  /// Where each plugin goes among the selected plugins when the user selects
  /// it (see [`plugin_ranks`]).
  ranks: &'a [usize],
  options_loader: Option<&'a dyn InitPluginOptionsLoader>,
  /// What holds back the version of a plugin that's written, which is the
  /// version to show the options of.
  age_cutoff: Option<&'a DependencyAgeCutoff>,
}

/// Prompts for the plugins to use, letting the user move them and open one to
/// change the files it formats and its options. What they chose for each
/// plugin is stored in `choices`. Provides the indexes of the selected plugins
/// in the order the user left them in, along with the plugins a config file
/// already has where they were among them.
///
/// The selected plugins are kept at the top in the order they're going to be
/// in the config file. That starts out under the plugins it already has, which
/// stay in their order while the selected ones can be moved in between them,
/// so the top of the list is what the config file's `plugins` ends up as.
///
/// The plugins the user prefers are only marked here. They're changed in a
/// prompt of their own (see [`prompt_for_init_preferences`]), and nothing done
/// in either changes the other.
async fn prompt_for_plugins(
  environment: &impl Environment,
  prompt: PluginPrompt<'_>,
  choices: &mut [PluginChoices],
  preferences: &InitPreferences,
) -> Result<Vec<usize>> {
  let is_already_configured = |index: usize| prompt.already_configured.get(index).copied().unwrap_or(false);
  let items = prompt
    .order
    .iter()
    .map(|&i| {
      let plugin = &prompt.plugins[i];
      let detail = plugin_prompt_detail(plugin, &choices[i], prompt.project_files, is_already_configured(i));
      let item = if is_already_configured(i) {
        MultiSelectItem::non_selectable(plugin.display_name().to_string())
      } else {
        MultiSelectItem::new(plugin.display_name().to_string(), prompt.defaults[i])
      };
      item
        .with_detail(detail)
        .with_id(i)
        .with_rank(prompt.ranks[i])
        .with_marked(is_preferred(preferences, plugin))
    })
    .collect::<Vec<_>>();
  let mut state = MultiSelectState::new(prompt.message, items);
  state.help = Some(format!(
    "space: toggle, \u{2192}: options, shift+\u{2191}/\u{2193}: move, type to filter, enter: finish, {}: preferred (dprint init --preferences)",
    PREFERRED_MARK
  ));
  state.features = MultiSelectFeatures {
    can_move: true,
    can_open: true,
    selected_first: true,
  };

  loop {
    match environment.get_multi_selection(&mut state)? {
      MultiSelectAction::Done => break,
      MultiSelectAction::Open(item_index) => {
        let index = state.items[item_index].id;
        let plugin = &prompt.plugins[index];
        let title = plugin.display_name();
        let options_source = prompt.options_loader.map(|loader| (loader, prompt.age_cutoff));
        edit_plugin_choices(environment, title, plugin, prompt.project_files, options_source, &mut choices[index]).await?;
        state.items[item_index].detail = plugin_prompt_detail(plugin, &choices[index], prompt.project_files, is_already_configured(index));
      }
    }
  }

  Ok(state.items.iter().filter(|item| item.is_selected).map(|item| item.id).collect())
}

/// The text the plugin prompt shows after a plugin the user prefers.
const PREFERRED_MARK: &str = "\u{2605}";

fn is_preferred(preferences: &InitPreferences, plugin: &InfoFilePluginInfo) -> bool {
  preferences.preferred_plugins.iter().any(|preferred| preferred.name == plugin.name)
}

/// The preferences after the user selected the plugins they prefer, which
/// are in the order they were shown in. The ones that were remembered that the
/// info file doesn't have stay after what they were after, since the user
/// wasn't shown those to say otherwise.
fn get_updated_preferences(
  old_preferences: &InitPreferences,
  plugins: &[InfoFilePluginInfo],
  choices: &[PluginChoices],
  preferred_indexes: &[usize],
) -> InitPreferences {
  let mut preferred_plugins = preferred_indexes
    .iter()
    .map(|&index| PreferredPlugin {
      name: plugins[index].name.clone(),
      excluded_file_extensions: choices[index].excluded_extensions.clone(),
      excluded_file_names: choices[index].excluded_file_names.clone(),
      config: choices[index].config.clone(),
    })
    .collect::<Vec<_>>();
  let old_plugins = &old_preferences.preferred_plugins;
  for (old_index, unknown_plugin) in old_plugins.iter().enumerate() {
    if plugins.iter().any(|plugin| plugin.name == unknown_plugin.name) {
      continue;
    }
    let index_after_previous = old_plugins[..old_index]
      .iter()
      .rev()
      .find_map(|previous| preferred_plugins.iter().position(|preferred| preferred.name == previous.name))
      .map(|index| index + 1);
    preferred_plugins.insert(index_after_previous.unwrap_or(0), unknown_plugin.clone());
  }
  InitPreferences { preferred_plugins }
}

/// Shows the files a plugin formats and its options so the user can change
/// them. The plugin is only downloaded to get its options once the user asks
/// for this, and leaving before that's done stops it. The options are those
/// of the version that would be written, which a dependency age holds back.
async fn edit_plugin_choices(
  environment: &impl Environment,
  title: &str,
  plugin: &InfoFilePluginInfo,
  project_files: &ProjectFiles,
  options_source: Option<(&dyn InitPluginOptionsLoader, Option<&DependencyAgeCutoff>)>,
  choices: &mut PluginChoices,
) -> Result<()> {
  let file_types = plugin_file_types(plugin);
  let mut editor = SettingsEditorState::new(title);
  editor.show_key_help = true;
  editor.checkboxes_title = "Files to format:".to_string();
  editor.checkboxes = file_types
    .iter()
    .map(|file_type| match file_type {
      PluginFileType::Extension(ext) => SettingsCheckbox {
        text: format!(".{}", ext),
        is_checked: !choices.excludes_extension(ext),
        is_highlighted: project_files.extensions.contains(&ext.to_lowercase()),
      },
      PluginFileType::FileName(name) => SettingsCheckbox {
        text: name.clone(),
        is_checked: !choices.excludes_file_name(name),
        is_highlighted: project_files.file_names.contains(name),
      },
    })
    .collect();
  editor.options_title = "Options:".to_string();
  let config = choices.config.clone();
  let pending_options = match options_source {
    Some((options_loader, age_cutoff)) => {
      editor.options = SettingsOptions::Loading("Downloading the plugin to get its options... (\u{2190} to cancel)".to_string());
      Some(
        async move {
          let plugin_entry = resolve_plugin_entry(plugin, age_cutoff, environment).await?;
          let mut options = options_loader.load(&plugin_entry).await?;
          apply_config_to_settings_options(&mut options, &config);
          Ok(options)
        }
        .boxed_local(),
      )
    }
    None => {
      // only what was already chosen is known
      let mut options = Vec::new();
      apply_config_to_settings_options(&mut options, &config);
      editor.options = SettingsOptions::Loaded(options);
      None
    }
  };
  environment.edit_settings(&mut editor, pending_options).await?;

  choices.excluded_extensions.clear();
  choices.excluded_file_names.clear();
  for (file_type, checkbox) in file_types.iter().zip(&editor.checkboxes) {
    if checkbox.is_checked {
      continue;
    }
    match file_type {
      PluginFileType::Extension(ext) => choices.excluded_extensions.push(ext.to_lowercase()),
      PluginFileType::FileName(name) => choices.excluded_file_names.push(name.clone()),
    }
  }
  // the options that didn't load (ex. the user left before they had) are left as they were
  if let SettingsOptions::Loaded(options) = &editor.options {
    choices.config = get_config_from_settings_options(options);
  }
  Ok(())
}

/// Reads the plugin info file, logging why the command is going to be less
/// helpful when it can't be read or is too new for this version of dprint.
async fn read_plugin_info_file(environment: &impl Environment) -> Option<InfoFile> {
  match read_info_file(environment).await {
    Ok(info) => {
      // ok to only check wasm here because the configuration file is only ever initialized with wasm plugins
      if wasm::PLUGIN_SYSTEM_SCHEMA_VERSION < info.plugin_system_schema_version {
        log_error!(
          environment,
          concat!(
            "You are using an old version of dprint so the latest plugins may not be suggested. ",
            "Consider upgrading to support new plugins. ",
            "Plugin system schema version is {}, latest is {}."
          ),
          wasm::PLUGIN_SYSTEM_SCHEMA_VERSION,
          info.plugin_system_schema_version,
        );
        None
      } else {
        Some(info)
      }
    }
    Err(err) => {
      log_error!(
        environment,
        concat!(
          "There was a problem getting the latest plugin info, so the latest plugins may not be suggested. ",
          "Error: {}"
        ),
        err,
      );
      None
    }
  }
}

/// A plugin `dprint init` decided to write to the config file.
struct SelectedPlugin {
  plugin: InfoFilePluginInfo,
  /// The plugin's config block.
  config: serde_json::Value,
  /// The text to write into the `plugins` array.
  entry: String,
}

/// Where to get a selected plugin from: its npm package when the info file
/// says it's distributed on npm (resolved from the registry, which is the
/// source of truth for the version), otherwise its url.
///
/// A registry lookup that fails falls back to the url with a warning — `init`
/// is best-effort and a starting point is more useful than no config file. The
/// one failure that isn't fallen back from is a minimum dependency age holding
/// every version back: the url names the plugin's newest release, so writing
/// it would hand back the very version the age ruled out.
///
/// The npm registry is resolved from the current directory: that's where the
/// config file lands in the normal case, and where the user ran the command in
/// any case.
async fn resolve_plugin_entry(plugin: &InfoFilePluginInfo, age_cutoff: Option<&DependencyAgeCutoff>, environment: &impl Environment) -> Result<String> {
  let options = ResolveNpmLatestOptions {
    force_checksum: false,
    base_dir: Some(environment.cwd()),
    minimum_dependency_age: age_cutoff.cloned(),
  };
  match plugin.resolve_npm(environment, options).await {
    Some(Ok(resolved)) => Ok(resolved.config_file_entry()),
    Some(Err(err)) if err.downcast_ref::<MinimumDependencyAgeError>().is_some() => Err(err),
    Some(Err(err)) => {
      log_warn!(
        environment,
        "Failed resolving the npm package for {}. Using its url instead. {:#}",
        plugin.name,
        err
      );
      Ok(plugin.full_url_no_wasm_checksum())
    }
    None => Ok(plugin.full_url_no_wasm_checksum()),
  }
}

/// Renders the config file for the selected plugins using jsonc-parser's CST,
/// which handles indentation, commas, and multi-line formatting for us.
fn render_config_file(selected_plugins: &[SelectedPlugin]) -> String {
  let root = CstRootNode::parse("{\n}", &Default::default()).unwrap();
  let root_obj = root.object_value_or_set();

  for SelectedPlugin { plugin, config, .. } in selected_plugins {
    if let Some(config_key) = &plugin.config_key
      && !config_key.is_empty()
    {
      let is_empty = config.as_object().is_none_or(|obj| obj.is_empty());
      let prop = root_obj.append(config_key, to_cst_input(config.clone()));
      // keep the brace of an empty block on its own line so there's a spot to add options
      if is_empty && let Some(object) = prop.value().and_then(|value| value.as_object()) {
        object.ensure_multiline();
      }
    }
  }

  let excludes = get_unique_items(
    selected_plugins
      .iter()
      .flat_map(|selected| selected.plugin.config_excludes.iter().cloned())
      .collect::<Vec<_>>(),
  );
  let excludes_prop = root_obj.append("excludes", CstInputValue::Array(excludes.iter().cloned().map(CstInputValue::String).collect()));
  if !excludes.is_empty()
    && let Some(array) = excludes_prop.value().and_then(|value| value.as_array())
  {
    array.ensure_multiline();
  }

  let entries = selected_plugins.iter().map(|selected| selected.entry.clone()).collect::<Vec<_>>();
  let plugins_prop = root_obj.append("plugins", CstInputValue::Array(entries.into_iter().map(CstInputValue::String).collect()));
  if let Some(array) = plugins_prop.value().and_then(|value| value.as_array()) {
    array.ensure_multiline();
  }

  format!("{root}\n")
}

/// Converts an owned `serde_json::Value` into the CST's input value type.
pub(super) fn to_cst_input(value: serde_json::Value) -> CstInputValue {
  use serde_json::Value;
  match value {
    Value::Null => CstInputValue::Null,
    Value::Bool(value) => CstInputValue::Bool(value),
    Value::Number(value) => CstInputValue::Number(value.to_string()),
    Value::String(value) => CstInputValue::String(value),
    Value::Array(values) => CstInputValue::Array(values.into_iter().map(to_cst_input).collect()),
    Value::Object(entries) => CstInputValue::Object(entries.into_iter().map(|(key, value)| (key, to_cst_input(value))).collect()),
  }
}

/// The files found while scanning the current directory, used to decide which
/// plugins to pre-select.
#[derive(Default)]
struct ProjectFiles {
  /// Lowercased file extensions without the leading dot (ex. `ts`, `json`).
  extensions: HashSet<String>,
  file_names: HashSet<String>,
}

/// Decides which plugins to pre-select based on the files in the current
/// directory, applying two priority rules in info.json order:
///
/// 1. Each present extension / file name is claimed by the first plugin that
///    matches it; a plugin is pre-selected when it claims at least one. So a
///    later plugin is still selected if it's the first to match some other
///    present extension (ex. one plugin owns `.ts`, a later one owns `.vue`).
/// 2. Two plugins that share a config key are never both selected — the earlier
///    one in the list wins.
///
/// An additive plugin sits outside the first rule: it formats a file in addition
/// to the plugin that claims it, so it's pre-selected whenever one of its files
/// is present and it never claims anything away from another plugin. The config
/// key rule still applies to it, because two config blocks with the same key
/// can't both be written to the config file.
///
/// A plugin matches via its own file extensions / file names or via any of its
/// config items (ex. `dprint-plugin-exec` declares no extensions of its own but
/// pre-selects when one of its command's file types is present).
///
/// A plugin marked `neverPreselect` sits outside both rules: it's skipped
/// entirely, so it's never pre-selected and never claims a file type or a
/// config key away from a plugin that would be. It's still offered in the
/// picker, just never suggested.
///
/// `already_configured` marks the plugins a config file already has (empty when
/// there's no config file yet). They claim their file types before anything
/// else and are never selected, so nothing is suggested for a file type that's
/// already handled. That includes a `neverPreselect` plugin: once it's in a
/// config file it handles its file types like any other.
///
/// The plugins the user prefers (`preferred_order`) go ahead of the others in
/// the order they prefer them, so they claim their files first. Being preferred
/// is the user asking for the plugin, so a preferred plugin is pre-selected
/// whenever one of its files is present that a config file doesn't already
/// handle, even when another preferred plugin claimed it or the plugin is
/// marked `neverPreselect`. The config key rule still applies.
///
/// The files the user said a plugin shouldn't format (`choices`) are left out
/// of what it matches.
fn compute_default_selections(
  plugins: &[InfoFilePluginInfo],
  project_files: &ProjectFiles,
  already_configured: &[bool],
  choices: &[PluginChoices],
  preferred_order: &[usize],
) -> Vec<bool> {
  let is_already_configured = |index: usize| already_configured.get(index).copied().unwrap_or(false);
  let mut claimed_extensions: HashSet<String> = HashSet::new();
  let mut claimed_file_names: HashSet<String> = HashSet::new();
  let mut used_config_keys: HashSet<&str> = HashSet::new();
  let mut selected = vec![false; plugins.len()];

  for (i, plugin) in plugins.iter().enumerate() {
    if !is_already_configured(i) {
      continue;
    }
    claimed_extensions.extend(present_extensions(plugin, &choices[i], project_files));
    claimed_file_names.extend(present_file_names(plugin, &choices[i], project_files));
    if let Some(config_key) = config_key(plugin) {
      used_config_keys.insert(config_key);
    }
  }
  let configured_extensions = claimed_extensions.clone();
  let configured_file_names = claimed_file_names.clone();

  let is_preferred = |index: usize| preferred_order.contains(&index);
  let other_indexes = (0..plugins.len()).filter(|&i| !is_preferred(i));
  for i in preferred_order.iter().copied().chain(other_indexes) {
    let plugin = &plugins[i];
    if is_already_configured(i) || plugin.never_preselect && !is_preferred(i) {
      continue;
    }
    // present extensions / file names that this plugin matches
    let present_extensions = present_extensions(plugin, &choices[i], project_files);
    let present_file_names = present_file_names(plugin, &choices[i], project_files);

    // an additive plugin is selected whenever one of its files is present,
    // without claiming it from the plugin that formats it
    let matches_present_file = !present_extensions.is_empty() || !present_file_names.is_empty();
    if plugin.additive && !matches_present_file {
      continue;
    }

    // select a claiming plugin only if it's the first to match at least one of
    // those, where a preferred plugin only loses out to a config file's plugins
    if !plugin.additive {
      let (claimed_extensions, claimed_file_names) = if is_preferred(i) {
        (&configured_extensions, &configured_file_names)
      } else {
        (&claimed_extensions, &claimed_file_names)
      };
      let claims_unclaimed =
        present_extensions.iter().any(|ext| !claimed_extensions.contains(ext)) || present_file_names.iter().any(|name| !claimed_file_names.contains(name));
      if !claims_unclaimed {
        continue;
      }
    }

    // never select two plugins that share a config key (earlier one wins)
    if let Some(config_key) = config_key(plugin)
      && !used_config_keys.insert(config_key)
    {
      continue;
    }

    selected[i] = true;
    if !plugin.additive {
      claimed_extensions.extend(present_extensions);
      claimed_file_names.extend(present_file_names);
    }
  }

  selected
}

/// The extensions found in the current directory that a plugin matches and
/// the user didn't say to leave out.
fn present_extensions(plugin: &InfoFilePluginInfo, choices: &PluginChoices, project_files: &ProjectFiles) -> Vec<String> {
  match_extensions(plugin)
    .into_iter()
    .filter(|ext| project_files.extensions.contains(ext) && !choices.excludes_extension(ext))
    .collect()
}

/// The file names found in the current directory that a plugin matches and
/// the user didn't say to leave out.
fn present_file_names(plugin: &InfoFilePluginInfo, choices: &PluginChoices, project_files: &ProjectFiles) -> Vec<String> {
  match_file_names(plugin)
    .into_iter()
    .filter(|name| project_files.file_names.contains(*name) && !choices.excludes_file_name(name))
    .map(|name| name.to_string())
    .collect()
}

/// The order plugins are displayed in: the ones a config file already has the
/// way it has them (`configured_order`, which is empty when there's no config
/// file yet), then the pre-selected ones, and then the rest. That's the
/// config file's `plugins` from the top down followed by what's left to pick
/// from. The plugins that aren't in a config file are in the order of their
/// rank in both groups (see [`plugin_ranks`]).
fn display_order(defaults: &[bool], configured_order: &[usize], preferred_order: &[usize]) -> Vec<usize> {
  let ranks = plugin_ranks(defaults.len(), preferred_order);
  let mut indexes = (0..defaults.len()).filter(|index| !configured_order.contains(index)).collect::<Vec<_>>();
  indexes.sort_by_key(|&index| ranks[index]);
  let selected = indexes.iter().copied().filter(|&i| defaults[i]);
  let unselected = indexes.iter().copied().filter(|&i| !defaults[i]);
  configured_order.iter().copied().chain(selected).chain(unselected).collect()
}

/// Where each plugin goes among the plugins that are selected the way it is:
/// the plugins the user prefers first in the order they prefer them, then the
/// rest in info.json order. Lowest goes first.
fn plugin_ranks(plugin_count: usize, preferred_order: &[usize]) -> Vec<usize> {
  let other_indexes = (0..plugin_count).filter(|index| !preferred_order.contains(index));
  let mut ranks = vec![0; plugin_count];
  for (rank, index) in preferred_order.iter().copied().chain(other_indexes).enumerate() {
    ranks[index] = rank;
  }
  ranks
}

/// A kind of file a plugin formats, which the user can say to leave out.
enum PluginFileType {
  /// A file extension without the leading dot, as the info file has it.
  Extension(String),
  FileName(String),
}

/// The files a plugin formats: its extensions then its file names, with the
/// ones of its config items after its own.
fn plugin_file_types(plugin: &InfoFilePluginInfo) -> Vec<PluginFileType> {
  let mut extensions: Vec<&String> = Vec::new();
  let config_item_extensions = plugin.config_items.iter().flat_map(|item| item.file_extensions.iter());
  for ext in plugin.file_extensions.iter().chain(config_item_extensions) {
    if !extensions.iter().any(|existing| existing.eq_ignore_ascii_case(ext)) {
      extensions.push(ext);
    }
  }
  let file_names = get_unique_items(match_file_names(plugin));
  extensions
    .into_iter()
    .map(|ext| PluginFileType::Extension(ext.clone()))
    .chain(file_names.into_iter().map(|name| PluginFileType::FileName(name.to_string())))
    .collect()
}

/// All the file extensions a plugin matches (its own plus its config items'),
/// lowercased to match the scan.
fn match_extensions(plugin: &InfoFilePluginInfo) -> Vec<String> {
  let mut extensions = plugin.file_extensions.iter().map(|ext| ext.to_lowercase()).collect::<Vec<_>>();
  for item in &plugin.config_items {
    extensions.extend(item.file_extensions.iter().map(|ext| ext.to_lowercase()));
  }
  extensions
}

/// All the file names a plugin matches (its own plus its config items').
fn match_file_names(plugin: &InfoFilePluginInfo) -> Vec<&str> {
  let mut file_names = plugin.file_names.iter().map(String::as_str).collect::<Vec<_>>();
  for item in &plugin.config_items {
    file_names.extend(item.file_names.iter().map(String::as_str));
  }
  file_names
}

/// A plugin's non-empty config key, if it has one.
fn config_key(plugin: &InfoFilePluginInfo) -> Option<&str> {
  plugin.config_key.as_deref().filter(|key| !key.is_empty())
}

/// Whether any of the extensions or file names are present in the current
/// directory. Extensions are matched case-insensitively, while file names are
/// matched exactly because their casing is significant (ex. `Cargo.toml`).
fn matches_project_files(file_extensions: &[String], file_names: &[String], project_files: &ProjectFiles) -> bool {
  file_extensions.iter().any(|ext| project_files.extensions.contains(&ext.to_lowercase()))
    || file_names.iter().any(|name| project_files.file_names.contains(name))
}

/// Everything shown dimmed after a plugin's name in the plugin prompt: the
/// files it formats followed by what else is worth knowing about it.
fn plugin_prompt_detail(
  plugin: &InfoFilePluginInfo,
  choices: &PluginChoices,
  project_files: &ProjectFiles,
  is_already_configured: bool,
) -> Vec<MultiSelectDetailPart> {
  let mut detail = plugin_display_detail(plugin, choices, project_files);
  let mut add_note = |text: &str| {
    let separator = if detail.is_empty() { "" } else { " " };
    detail.push(MultiSelectDetailPart::new(format!("{}\u{2014} {}", separator, text)));
  };
  if choices.is_customized() {
    add_note("customized");
  }
  if is_already_configured {
    // shown with its extensions like the selectable items so filtering by a
    // file type still surfaces the plugin that already handles it
    add_note("already in config");
  }
  detail
}

/// The text shown dimmed after a plugin's name in the selection list. The files
/// it formats are listed so unfamiliar plugins are easier to tell apart, with
/// the ones found in the current directory highlighted whether or not the
/// plugin is pre-selected. An additive plugin is marked because it runs
/// alongside another plugin rather than replacing it.
fn plugin_display_detail(plugin: &InfoFilePluginInfo, choices: &PluginChoices, project_files: &ProjectFiles) -> Vec<MultiSelectDetailPart> {
  let mut details = display_file_matches(plugin, choices, project_files);
  if plugin.additive {
    details.push(MultiSelectDetailPart::new("runs in addition to other plugins"));
  }
  if details.is_empty() {
    return Vec::new();
  }
  let mut parts = Vec::with_capacity(details.len() * 2 + 1);
  parts.push(MultiSelectDetailPart::new("("));
  for (i, detail) in details.into_iter().enumerate() {
    if i > 0 {
      parts.push(MultiSelectDetailPart::new(", "));
    }
    parts.push(detail);
  }
  parts.push(MultiSelectDetailPart::new(")"));
  parts
}

/// The files to show beside a plugin's name: its extensions, falling back to the
/// extensions of its config items, then to the file names it matches (ex. a
/// plugin that only formats `package.json` declares no extensions at all). The
/// ones found in the current directory are highlighted, and the ones the user
/// said to leave out aren't shown.
fn display_file_matches(plugin: &InfoFilePluginInfo, choices: &PluginChoices, project_files: &ProjectFiles) -> Vec<MultiSelectDetailPart> {
  let extensions = display_extensions(plugin);
  if !extensions.is_empty() {
    return extensions
      .iter()
      .filter(|ext| !choices.excludes_extension(ext))
      .map(|ext| MultiSelectDetailPart {
        text: format!(".{}", ext),
        is_highlighted: project_files.extensions.contains(&ext.to_lowercase()),
      })
      .collect();
  }
  get_unique_items(match_file_names(plugin).into_iter().map(ToOwned::to_owned).collect())
    .into_iter()
    .filter(|name| !choices.excludes_file_name(name))
    .map(|name| MultiSelectDetailPart {
      is_highlighted: project_files.file_names.contains(&name),
      text: name,
    })
    .collect()
}

/// The extensions to show beside a plugin's name, falling back to the extensions
/// of its config items when the plugin declares none of its own.
fn display_extensions(plugin: &InfoFilePluginInfo) -> Vec<String> {
  if !plugin.file_extensions.is_empty() {
    return plugin.file_extensions.clone();
  }
  let mut extensions = Vec::new();
  for item in &plugin.config_items {
    for ext in &item.file_extensions {
      if !extensions.contains(ext) {
        extensions.push(ext.clone());
      }
    }
  }
  extensions
}

/// Builds the config block for a selected plugin: starts from its `defaultConfig`
/// and merges in each config item whose files are present in the current
/// directory. Objects merge recursively and arrays are concatenated, so command
/// lists (ex. `dprint-plugin-exec`) accumulate.
///
/// A config item isn't merged in when the user said to leave out the files
/// that would have brought it in. What the user chose for the plugin goes on
/// top of all that (see [`apply_choices_to_config`]).
///
/// The order keys appear in the output relies on `serde_json`'s `preserve_order`
/// feature (and jsonc-parser's), so config reads in the same order it's declared.
fn build_plugin_config(plugin: &InfoFilePluginInfo, project_files: &ProjectFiles, choices: &PluginChoices) -> serde_json::Value {
  let mut config = match plugin.default_config.clone() {
    Some(value @ serde_json::Value::Object(_)) => value,
    _ => serde_json::Value::Object(Default::default()),
  };
  for item in &plugin.config_items {
    let file_extensions = item
      .file_extensions
      .iter()
      .filter(|ext| !choices.excludes_extension(ext))
      .cloned()
      .collect::<Vec<_>>();
    let file_names = item
      .file_names
      .iter()
      .filter(|name| !choices.excludes_file_name(name))
      .cloned()
      .collect::<Vec<_>>();
    if matches_project_files(&file_extensions, &file_names, project_files) {
      deep_merge(&mut config, item.config.clone());
    }
  }
  if let serde_json::Value::Object(config) = &mut config {
    apply_choices_to_config(config, plugin, choices);
  }
  config
}

/// Applies what the user chose for a plugin to its config block. An option
/// they set replaces what's there rather than being merged with it, so what
/// gets written is exactly what they chose. The files they said to leave out
/// that the plugin matches on its own become negated `associations`, which is
/// how the CLI is told a plugin shouldn't format something it otherwise would.
fn apply_choices_to_config(config: &mut serde_json::Map<String, serde_json::Value>, plugin: &InfoFilePluginInfo, choices: &PluginChoices) {
  for (name, value) in &choices.config {
    config.insert(name.clone(), value.clone());
  }

  let excluded_extensions = plugin.file_extensions.iter().filter(|ext| choices.excludes_extension(ext));
  let excluded_file_names = plugin.file_names.iter().filter(|name| choices.excludes_file_name(name));
  let patterns = excluded_extensions
    .map(|ext| format!("!**/*.{}", ext))
    .chain(excluded_file_names.map(|name| format!("!**/{}", name)))
    .map(serde_json::Value::String)
    .collect::<Vec<_>>();
  if patterns.is_empty() {
    return;
  }
  let mut associations = match config.remove("associations") {
    Some(serde_json::Value::Array(associations)) => associations,
    // a single pattern doesn't need to be in an array
    Some(association @ serde_json::Value::String(_)) => vec![association],
    _ => Vec::new(),
  };
  for pattern in patterns {
    if !associations.contains(&pattern) {
      associations.push(pattern);
    }
  }
  config.insert("associations".to_string(), serde_json::Value::Array(associations));
}

/// Merges `other` into `base`: objects merge recursively, arrays are
/// concatenated, and anything else overwrites.
fn deep_merge(base: &mut serde_json::Value, other: serde_json::Value) {
  use serde_json::Value;
  match other {
    Value::Object(other_map) if base.is_object() => {
      let base_map = base.as_object_mut().unwrap();
      for (key, value) in other_map {
        match base_map.get_mut(&key) {
          Some(existing) => deep_merge(existing, value),
          None => {
            base_map.insert(key, value);
          }
        }
      }
    }
    Value::Array(mut other_items) if base.is_array() => {
      base.as_array_mut().unwrap().append(&mut other_items);
    }
    other => *base = other,
  }
}

/// Scans the current directory for files in order to decide which plugins to
/// pre-select. Files and directories the gitignore excludes are skipped, as
/// are the directories in [`is_ignored_dir`]. The scan is bounded by
/// [`MAX_SCAN_FILES`] and [`MAX_SCAN_DIRS`] to stay fast in large repositories
/// and best-effort ignores errors.
fn scan_project_files(environment: &impl Environment) -> ProjectFiles {
  let mut extensions = HashSet::new();
  let mut file_names = HashSet::new();
  let mut files_remaining = MAX_SCAN_FILES;
  let mut dirs_remaining = MAX_SCAN_DIRS;
  let mut pending_dirs = VecDeque::from([environment.cwd().into_path_buf()]);
  let mut gitignores = GitIgnoreTree::new(
    environment.clone(),
    GitIgnoreTreeOptions {
      include_paths: Vec::new(),
      global_gitignore_lines: resolve_global_gitignore_lines(environment),
      detect_changes: false,
    },
  );

  'outer: while let Some(dir) = pending_dirs.pop_front() {
    if dirs_remaining == 0 {
      break;
    }
    dirs_remaining -= 1;
    let Ok(entries) = environment.dir_info(&dir) else {
      continue; // best-effort: skip directories we can't read
    };
    let gitignore = gitignores.get_resolved_git_ignore_for_dir_children(&dir, DirEntriesHint::from_dir_entries(&entries));
    let is_ignored = |path: &PathBuf, is_dir: bool| gitignore.as_ref().is_some_and(|gitignore| gitignore.is_ignored(path, is_dir));
    for entry in entries {
      match entry {
        DirEntry::Directory(path) => {
          let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
          };
          if !is_ignored_dir(name) && !is_ignored(&path, true) {
            pending_dirs.push_back(path);
          }
        }
        DirEntry::File { path } => {
          if is_ignored(&path, false) {
            continue;
          }
          if files_remaining == 0 {
            break 'outer;
          }
          files_remaining -= 1;
          if let Some(name) = path.file_name() {
            file_names.insert(name.to_string_lossy().into_owned());
          }
          if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            extensions.insert(ext.to_lowercase());
          }
        }
      }
    }
  }

  ProjectFiles { extensions, file_names }
}

/// Directories that are skipped while scanning. Hidden directories other than
/// `.git` are scanned because they may hold files worth formatting (ex.
/// `.github/workflows`), and the ones that don't are usually gitignored.
fn is_ignored_dir(name: &str) -> bool {
  matches!(name, ".git" | "node_modules" | "target" | "vendor" | "dist" | "build" | "out" | "bin" | "obj")
}

/// Gets the unique items in the vector in the same order
fn get_unique_items<T>(vec: Vec<T>) -> Vec<T>
where
  T: PartialEq,
{
  let mut new_vec = Vec::new();

  for item in vec {
    if !new_vec.contains(&item) {
      new_vec.push(item);
    }
  }

  new_vec
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::environment::TestInfoFileConfigItem;
  use crate::environment::TestInfoFileMatch;
  use crate::environment::TestInfoFileNpm;
  use crate::environment::TestInfoFilePlugin;
  use crate::plugins::InfoFileConfigItem;
  use crate::utils::SettingsOption;
  use crate::utils::SettingsOptionKind;
  use pretty_assertions::assert_eq;

  fn exec_info_plugin() -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: "dprint-plugin-exec".to_string(),
      version: "0.5.0".to_string(),
      url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
      config_key: Some("exec".to_string()),
      file_extensions: vec![],
      config_excludes: vec![],
      checksum: Some("checksum".to_string()),
      // ${configDir} is an exec-plugin runtime token, emitted verbatim (not expanded here)
      default_config: Some(serde_json::json!({ "cwd": "${configDir}" })),
      config_items: vec![
        TestInfoFileConfigItem {
          file_match: TestInfoFileMatch {
            file_extensions: vec!["rs".to_string()],
            file_names: vec![],
          },
          config: serde_json::json!({ "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
        },
        TestInfoFileConfigItem {
          file_match: TestInfoFileMatch {
            file_extensions: vec!["go".to_string()],
            file_names: vec![],
          },
          config: serde_json::json!({ "commands": [{ "command": "gofmt", "exts": ["go"] }] }),
        },
      ],
      ..Default::default()
    }
  }

  fn info_plugin_with_extensions(file_extensions: Vec<&str>, config_item_extensions: Vec<Vec<&str>>) -> InfoFilePluginInfo {
    InfoFilePluginInfo {
      name: "dprint-plugin-exec".to_string(),
      display_name: None,
      version: "0.5.0".to_string(),
      url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
      config_key: Some("exec".to_string()),
      file_extensions: file_extensions.into_iter().map(String::from).collect(),
      file_names: vec![],
      config_excludes: vec![],
      checksum: None,
      additive: false,
      never_preselect: false,
      npm: None,
      default_config: None,
      config_items: config_item_extensions
        .into_iter()
        .map(|extensions| InfoFileConfigItem {
          file_extensions: extensions.into_iter().map(String::from).collect(),
          file_names: vec![],
          config: serde_json::Value::Object(Default::default()),
        })
        .collect(),
    }
  }

  #[test]
  fn files_left_out_are_added_to_the_associations_that_are_there() {
    let plugin = info_plugin_with_extensions(vec!["ts", "yaml"], vec![]);
    let choices = PluginChoices {
      excluded_extensions: vec!["yaml".to_string()],
      ..Default::default()
    };
    let apply = |config: serde_json::Value| {
      let mut config = config.as_object().unwrap().clone();
      apply_choices_to_config(&mut config, &plugin, &choices);
      serde_json::Value::Object(config)
    };
    assert_eq!(apply(serde_json::json!({})), serde_json::json!({ "associations": ["!**/*.yaml"] }));
    // a single pattern doesn't have to be in an array
    assert_eq!(
      apply(serde_json::json!({ "associations": "**/*.foo" })),
      serde_json::json!({ "associations": ["**/*.foo", "!**/*.yaml"] })
    );
    assert_eq!(
      apply(serde_json::json!({ "associations": ["**/*.foo", "!**/*.yaml"] })),
      serde_json::json!({ "associations": ["**/*.foo", "!**/*.yaml"] })
    );
    // nothing is left out
    let choices = PluginChoices::default();
    let mut config = serde_json::json!({ "associations": "**/*.foo" }).as_object().unwrap().clone();
    apply_choices_to_config(&mut config, &plugin, &choices);
    assert_eq!(serde_json::Value::Object(config), serde_json::json!({ "associations": "**/*.foo" }));
  }

  #[test]
  fn plugin_display_detail_falls_back_to_config_item_extensions() {
    // a plugin's own extensions take precedence
    let plugin = info_plugin_with_extensions(vec!["ts", "tsx"], vec![vec!["rs"]]);
    assert_eq!(display_detail_text(&plugin, &project_files(&[], &[])), "(.ts, .tsx)");
    // otherwise the extensions are derived from the config items, deduplicated in order
    let plugin = info_plugin_with_extensions(vec![], vec![vec!["rs"], vec!["go", "rs"]]);
    assert_eq!(display_detail_text(&plugin, &project_files(&[], &[])), "(.rs, .go)");
    // nothing to show
    let plugin = info_plugin_with_extensions(vec![], vec![]);
    assert_eq!(display_detail_text(&plugin, &project_files(&[], &[])), "");
  }

  #[test]
  fn plugin_display_detail_highlights_files_found_in_the_current_directory() {
    // extensions are matched case-insensitively
    let plugin = info_plugin_with_extensions(vec!["ts", "TSX", "mts"], vec![]);
    assert_eq!(
      display_detail_text(&plugin, &project_files(&["ts", "tsx", "json"], &[])),
      "(*.ts*, *.TSX*, .mts)"
    );
    // file names are matched exactly
    let mut plugin = info_plugin_with_extensions(vec![], vec![]);
    plugin.file_names = vec!["Dockerfile".to_string(), "Containerfile".to_string()];
    assert_eq!(
      display_detail_text(&plugin, &project_files(&["dockerfile"], &["Dockerfile"])),
      "(*Dockerfile*, Containerfile)"
    );
  }

  /// The detail shown after a plugin's name, with the highlighted parts
  /// surrounded by asterisks.
  fn display_detail_text(plugin: &InfoFilePluginInfo, project_files: &ProjectFiles) -> String {
    plugin_display_detail(plugin, &Default::default(), project_files)
      .into_iter()
      .map(|part| if part.is_highlighted { format!("*{}*", part.text) } else { part.text })
      .collect()
  }

  fn project_files(extensions: &[&str], file_names: &[&str]) -> ProjectFiles {
    ProjectFiles {
      extensions: extensions.iter().map(|ext| ext.to_string()).collect(),
      file_names: file_names.iter().map(|name| name.to_string()).collect(),
    }
  }

  #[test]
  fn should_scaffold_config_from_file_name_match() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-exec".to_string(),
          version: "0.5.0".to_string(),
          url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
          config_key: Some("exec".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          checksum: Some("checksum".to_string()),
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec![],
              file_names: vec!["Makefile".to_string()],
            },
            config: serde_json::json!({ "commands": [{ "command": "make", "exts": [] }] }),
          }],
          ..Default::default()
        });
      })
      .write_file("/Makefile", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "exec": {
    "commands": [
      {
        "command": "make",
        "exts": []
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_let_config_item_override_default_config() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-exec".to_string(),
          version: "0.5.0".to_string(),
          url: "https://plugins.dprint.dev/exec-0.5.0.json".to_string(),
          config_key: Some("exec".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          checksum: Some("checksum".to_string()),
          default_config: Some(serde_json::json!({ "cwd": "default", "indentWidth": 2 })),
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec!["rs".to_string()],
              file_names: vec![],
            },
            config: serde_json::json!({ "cwd": "${configDir}", "commands": [{ "command": "rustfmt", "exts": ["rs"] }] }),
          }],
          ..Default::default()
        });
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // the config item overrides cwd, keeps indentWidth, and appends commands
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "indentWidth": 2,
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  fn json_info_plugin() -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: "dprint-plugin-json".to_string(),
      version: "0.19.2".to_string(),
      url: "https://plugins.dprint.dev/json-0.19.2.wasm".to_string(),
      config_key: Some("json".to_string()),
      file_extensions: vec!["json".to_string()],
      config_excludes: vec![],
      ..Default::default()
    }
  }

  fn additive_info_plugin() -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: "dprint-plugin-additive".to_string(),
      version: "0.1.0".to_string(),
      url: "https://plugins.dprint.dev/additive-0.1.0.wasm".to_string(),
      config_key: Some("additivePlugin".to_string()),
      file_extensions: vec![],
      file_names: Some(vec!["package.json".to_string()]),
      config_excludes: vec![],
      additive: true,
      ..Default::default()
    }
  }

  #[test]
  fn should_pre_select_additive_plugin_alongside_the_plugin_that_claims_the_file() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(json_info_plugin()).add_plugin(additive_info_plugin());
      })
      .write_file("/package.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      // the additive plugin matching package.json doesn't stop the json plugin
      // from being pre-selected for it
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "additivePlugin": {
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/json-0.19.2.wasm",
    "https://plugins.dprint.dev/additive-0.1.0.wasm"
  ]
}
"#
      );
    });
  }

  #[test]
  fn should_not_let_an_additive_plugin_claim_files_from_a_later_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // the additive plugin matches the same extension and is listed first
        let mut additive = additive_info_plugin();
        additive.file_extensions = vec!["json".to_string()];
        info.add_plugin(additive).add_plugin(json_info_plugin());
      })
      .write_file("/data.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      // the json plugin is still pre-selected for the .json file
      assert!(text.contains("json-0.19.2.wasm"), "{text}");
      assert!(text.contains("additive-0.1.0.wasm"), "{text}");
    });
  }

  #[test]
  fn should_not_pre_select_an_additive_plugin_that_shares_a_config_key() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut additive = additive_info_plugin();
        // the same config key as the json plugin, which can only be written once
        additive.config_key = Some("json".to_string());
        info.add_plugin(json_info_plugin()).add_plugin(additive);
      })
      .write_file("/package.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      // the earlier plugin wins the config key, so the additive one is left out
      assert!(text.contains("json-0.19.2.wasm"), "{text}");
      assert!(!text.contains("additive-0.1.0.wasm"), "{text}");
    });
  }

  #[test]
  fn should_not_pre_select_additive_plugin_when_its_files_arent_present() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(json_info_plugin()).add_plugin(additive_info_plugin());
      })
      .write_file("/data.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      assert!(!text.contains("additivePlugin"), "{text}");
      assert!(text.contains("json-0.19.2.wasm"), "{text}");
    });
  }

  #[test]
  fn plugin_display_detail_marks_additive_plugins() {
    let mut plugin = info_plugin_with_extensions(vec![], vec![]);
    plugin.name = "dprint-plugin-additive".to_string();
    plugin.additive = true;
    // a plugin with no extensions of its own shows the file names it matches
    plugin.file_names = vec!["package.json".to_string()];
    assert_eq!(
      display_detail_text(&plugin, &project_files(&[], &[])),
      "(package.json, runs in addition to other plugins)"
    );
    plugin.file_extensions = vec!["json".to_string()];
    assert_eq!(
      display_detail_text(&plugin, &project_files(&[], &[])),
      "(.json, runs in addition to other plugins)"
    );
  }

  #[test]
  fn should_not_pre_select_a_never_preselect_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("a", "a", &["txt"]);
        plugin.never_preselect = true;
        info.add_plugin(plugin);
      })
      .write_file("/notes.txt", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
        // it matches the .txt file, but it's only offered rather than suggested
        assert_eq!(environment.take_multi_selection_items(), vec!["[ ] a (.txt)"]);
        assert!(!text.contains("a-1.0.0.wasm"), "{text}");
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_let_a_never_preselect_plugin_claim_anything_from_a_later_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // the two plugins compete for the same file type and the same config key
        let mut plugin = wasm_plugin("a", "shared", &["txt"]);
        plugin.never_preselect = true;
        info.add_plugin(plugin).add_plugin(wasm_plugin("b", "shared", &["txt"]));
      })
      .write_file("/notes.txt", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      // `b` is still pre-selected for the .txt file
      assert!(text.contains("b-1.0.0.wasm"), "{text}");
      assert!(!text.contains("a-1.0.0.wasm"), "{text}");
    });
  }

  #[test]
  fn should_let_a_never_preselect_plugin_a_config_file_has_claim_its_file_types() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("a", "a", &["txt"]);
        plugin.never_preselect = true;
        info.add_plugin(plugin).add_plugin(wasm_plugin("b", "b", &["txt"]));
      })
      .write_file("/notes.txt", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugins = get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        // `a` handles .txt once it's in the config file, so `b` isn't suggested
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] a (.txt) — already in config (locked)", "[ ] b (.txt)"]
        );
        let InitPluginsToAdd::Plugins(plugins) = plugins else {
          unreachable!();
        };
        let entries = plugins.into_iter().map(|plugin| plugin.entry).collect::<Vec<_>>();
        assert!(entries.is_empty());
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_auto_select_exec_non_interactive() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      // exec is auto-selected by the .rs file with no prompt
      assert!(text.contains("\"command\": \"rustfmt\""), "{text}");
      assert!(text.contains("exec-0.5.0.json@checksum"), "{text}");
      assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());
    });
  }

  #[test]
  fn should_scaffold_exec_config_from_matched_file() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // exec is pre-selected by the .rs file; cwd comes from defaultConfig and the
      // rustfmt command from the matching config item (gofmt is not added)
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_concatenate_config_items_in_info_order() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .write_file("/lib.go", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // both commands are appended in info.json order
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}",
    "commands": [
      {
        "command": "rustfmt",
        "exts": ["rs"]
      },
      {
        "command": "gofmt",
        "exts": ["go"]
      }
    ]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_apply_only_default_config_when_no_items_match() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .build();
    // manually select exec even though no matching files are present
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "exec": {
    "cwd": "${configDir}"
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/exec-0.5.0.json@checksum"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn deep_merge_combines_objects_and_arrays() {
    let mut base = serde_json::json!({ "a": 1, "list": [1, 2], "nested": { "x": 1 } });
    deep_merge(&mut base, serde_json::json!({ "a": 2, "list": [3], "nested": { "y": 2 }, "b": 3 }));
    assert_eq!(
      base,
      // scalars override, arrays concatenate, objects merge, new keys insert
      serde_json::json!({ "a": 2, "list": [1, 2, 3], "nested": { "x": 1, "y": 2 }, "b": 3 })
    );
  }

  fn wasm_plugin(name: &str, config_key: &str, extensions: &[&str]) -> TestInfoFilePlugin {
    TestInfoFilePlugin {
      name: name.to_string(),
      version: "1.0.0".to_string(),
      url: format!("https://plugins.dprint.dev/{}-1.0.0.wasm", name),
      config_key: Some(config_key.to_string()),
      file_extensions: extensions.iter().map(|e| e.to_string()).collect(),
      config_excludes: vec![],
      ..Default::default()
    }
  }

  #[test]
  fn should_show_plugin_display_names() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("dprint_plugin_a", "a", &["ts"]);
        plugin.display_name = Some("someone/a".to_string());
        info.add_plugin(plugin).add_plugin(wasm_plugin("b", "b", &["md"]));
      })
      .write_file("/main.ts", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
        // the display name is only for showing the plugin
        assert_eq!(environment.take_multi_selection_items(), vec!["[x] someone/a (.ts)", "[ ] b (.md)"]);
        assert!(text.contains("dprint_plugin_a-1.0.0.wasm"), "{text}");
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_get_plugins_to_add_to_an_existing_config_file() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(wasm_plugin("b", "b", &["json"]))
          .add_plugin(wasm_plugin("c", "c", &["md"]));
      })
      .write_file("/main.ts", "")
      .write_file("/data.json", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugins = get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        // `b` is pre-selected because of the .json file, `a` is only shown for
        // context since the config file already has it (even though .ts matched)
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] a (.ts) — already in config (locked)", "[x] b (.json)", "[ ] c (.md)"]
        );
        let InitPluginsToAdd::Plugins(plugins) = plugins else {
          unreachable!();
        };
        let entries = plugins.into_iter().map(|plugin| plugin.entry).collect::<Vec<_>>();
        assert_eq!(entries, vec!["https://plugins.dprint.dev/b-1.0.0.wasm".to_string()]);
        assert_eq!(environment.take_stderr_messages(), vec!["Select plugins to add:"]);
      }
    });
  }

  #[test]
  fn should_not_suggest_a_plugin_for_a_file_type_an_existing_plugin_handles() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugins = get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        // `a` already handles .ts, so `b` isn't pre-selected
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] a (.ts) — already in config (locked)", "[ ] b (.ts)"]
        );
        let InitPluginsToAdd::Plugins(plugins) = plugins else {
          unreachable!();
        };
        let entries = plugins.into_iter().map(|plugin| plugin.entry).collect::<Vec<_>>();
        assert!(entries.is_empty());
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_prompt_when_the_config_file_has_every_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugins = get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        assert!(matches!(plugins, InitPluginsToAdd::AllPluginsConfigured));
        assert!(environment.take_multi_selection_items().is_empty());
      }
    });
  }

  #[test]
  fn display_order_lists_selected_first() {
    assert_eq!(display_order(&[false, true, false, true], &[], &[]), vec![1, 3, 0, 2]);
    assert_eq!(display_order(&[true, true], &[], &[]), vec![0, 1]);
    assert_eq!(display_order(&[false, false, false], &[], &[]), vec![0, 1, 2]);
    assert_eq!(display_order(&[], &[], &[]), Vec::<usize>::new());
    // the plugins already in the config file go last
    assert_eq!(display_order(&[false, true, false, false], &[3, 0], &[]), vec![3, 0, 1, 2]);
  }

  #[test]
  fn should_select_only_the_first_plugin_for_a_shared_extension() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/file.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // only the first plugin claims `ts`
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_select_both_plugins_on_partial_extension_overlap() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // `a` owns `ts`; `b` shares `ts` but also owns `vue`, so it's still selected
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(wasm_plugin("b", "b", &["ts", "vue"]));
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_select_two_plugins_with_the_same_config_key() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        // both use config key "markup"; `b` would otherwise be selected for `vue`
        info
          .add_plugin(wasm_plugin("a", "markup", &["ts"]))
          .add_plugin(wasm_plugin("b", "markup", &["ts", "vue"]));
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_write_plugins_in_the_order_they_are_shown_in() {
    // `b` (index 1) is the sole pre-selected plugin, so it's shown first; selecting
    // both from the reordered list writes them the way the user saw them, since
    // the order is theirs to change
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["js"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/file.ts", "")
      .build();
    environment.set_multi_selection_result(vec![0, 1]); // both items, in display order
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      let a_pos = text.find("a-1.0.0.wasm").expect("a present");
      let b_pos = text.find("b-1.0.0.wasm").expect("b present");
      // b was displayed first, so it's written first
      assert!(b_pos < a_pos, "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_select_keyless_plugins_for_distinct_extensions() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            config_key: None,
            ..wasm_plugin("a", "", &["ts"])
          })
          .add_plugin(TestInfoFilePlugin {
            config_key: None,
            ..wasm_plugin("b", "", &["vue"])
          });
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // plugins without a config key never collide on it
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_select_plugins_with_shared_config_key_matched_via_config_items() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "shared", &["ts"])).add_plugin(TestInfoFilePlugin {
          name: "b".to_string(),
          version: "1.0.0".to_string(),
          url: "https://plugins.dprint.dev/b-1.0.0.wasm".to_string(),
          config_key: Some("shared".to_string()),
          file_extensions: vec![],
          config_excludes: vec![],
          config_items: vec![TestInfoFileConfigItem {
            file_match: TestInfoFileMatch {
              file_extensions: vec!["rs".to_string()],
              file_names: vec![],
            },
            config: serde_json::json!({}),
          }],
          ..Default::default()
        });
      })
      .write_file("/file.ts", "")
      .write_file("/file.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // `b` matches `.rs` via a config item but shares the "shared" key with `a`
      assert!(text.contains("a-1.0.0.wasm"), "{text}");
      assert!(!text.contains("b-1.0.0.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  /// Serves an npm package whose latest version is newer than the info file's,
  /// to show which one is written.
  fn add_npm_package(builder: &mut TestEnvironmentBuilder, name: &str, file_name: &str) -> String {
    let tarball = crate::test_helpers::create_test_npm_tarball(&[(&format!("package/{}", file_name), b"")]);
    let checksum = crate::utils::get_sha256_checksum(&tarball);
    builder
      .add_remote_file_bytes(
        &format!("https://registry.npmjs.org/{}", name),
        serde_json::json!({
          "dist-tags": { "latest": "1.1.0" },
          "versions": { "1.1.0": { "dist": { "tarball": format!("https://registry.npmjs.org/{}/-/pkg-1.1.0.tgz", name) } } }
        })
        .to_string()
        .into_bytes(),
      )
      .add_remote_file_bytes(&format!("https://registry.npmjs.org/{}/-/pkg-1.1.0.tgz", name), tarball);
    checksum
  }

  #[test]
  fn should_initialize_with_npm_specifiers() {
    let mut builder = TestEnvironmentBuilder::new();
    // the version written comes from the registry (1.1.0), not the info file (1.0.0)
    add_npm_package(&mut builder, "@dprint/a", "plugin.wasm");
    let b_checksum = add_npm_package(&mut builder, "@dprint/b", "plugin.json");
    let environment = builder
      .with_info_file(|info| {
        info
          .add_plugin(TestInfoFilePlugin {
            npm: Some(TestInfoFileNpm {
              name: "@dprint/a".to_string(),
              ..Default::default()
            }),
            ..wasm_plugin("a", "a", &["ts"])
          })
          // a process plugin also carries the checksum of its npm package
          .add_plugin(TestInfoFilePlugin {
            name: "b".to_string(),
            version: "1.0.0".to_string(),
            url: "https://plugins.dprint.dev/b-1.0.0.json".to_string(),
            file_extensions: vec!["vue".to_string()],
            config_excludes: vec![],
            checksum: Some("url-checksum".to_string()),
            npm: Some(TestInfoFileNpm {
              name: "@dprint/b".to_string(),
              ..Default::default()
            }),
            ..Default::default()
          })
          // a plugin that isn't on npm keeps its url
          .add_plugin(TestInfoFilePlugin {
            name: "c".to_string(),
            version: "1.0.0".to_string(),
            url: "https://plugins.dprint.dev/c-1.0.0.json".to_string(),
            file_extensions: vec!["rs".to_string()],
            config_excludes: vec![],
            checksum: Some("url-checksum".to_string()),
            ..Default::default()
          });
      })
      .write_file("/file.ts", "")
      .write_file("/file.vue", "")
      .write_file("/file.rs", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        format!(
          r#"{{
  "a": {{
  }},
  "excludes": [],
  "plugins": [
    "npm:@dprint/a@1.1.0",
    "npm:@dprint/b@1.1.0/plugin.json@{}",
    "https://plugins.dprint.dev/c-1.0.0.json@url-checksum"
  ]
}}
"#,
          b_checksum
        )
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_fall_back_to_the_url_when_the_npm_registry_is_unreachable() {
    // init is best-effort — a starting point beats no config file
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          npm: Some(TestInfoFileNpm {
            name: "@dprint/a".to_string(),
            ..Default::default()
          }),
          ..wasm_plugin("a", "a", &["ts"])
        });
      })
      .write_file("/file.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("\"https://plugins.dprint.dev/a-1.0.0.wasm\""), "{text}");
      let mut expected_messages = get_standard_logged_messages();
      expected_messages.push("Failed resolving the npm package for a. Using its url instead. Failed to fetch npm packument for @dprint/a: Error downloading https://registry.npmjs.org/@dprint/a - 404 Not Found");
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  #[test]
  fn should_hold_an_npm_plugin_back_to_a_version_old_enough() {
    let environment = age_test_builder("2099-12-31T00:00:00Z").build();
    environment.set_fs_time(AGE_TEST_NOW);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, age_test_options("P3D")).await.unwrap();
      // 1.1.0 landed the day before "now", so the release before it is written
      assert!(text.contains("\"npm:@dprint/a@1.0.0\""), "{text}");
      let mut expected_messages = get_standard_logged_messages_no_plugin_selection();
      expected_messages.push("Using @dprint/a 1.0.0 instead of 1.1.0, which is newer than the minimum dependency age allows (--minimum-dependency-age P3D). Pass --minimum-dependency-age=0 to use the latest version.");
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  #[test]
  fn should_error_when_no_npm_version_is_old_enough() {
    // init falls back to a plugin's url when the registry can't be reached,
    // but not here: the url names the newest release, so falling back would
    // write the very version the age ruled out
    let environment = age_test_builder("2099-12-31T00:00:00Z").build();
    environment.set_fs_time(AGE_TEST_NOW);
    environment.clone().run_in_runtime(async move {
      let err = get_init_config_file_text(&environment, age_test_options("P400D")).await.err().unwrap();
      assert_eq!(
        err.to_string(),
        "No version of @dprint/a at or below 1.1.0 is old enough for the minimum dependency age (--minimum-dependency-age P400D)."
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages_no_plugin_selection());
    });
  }

  /// 2100-01-01T00:00:00Z — the "now" the minimum dependency age tests run at.
  const AGE_TEST_NOW: u64 = 4102444800;

  /// An environment with a single npm distributed plugin whose package has an
  /// old 1.0.0 and a 1.1.0 published at `latest_time`.
  fn age_test_builder(latest_time: &str) -> TestEnvironmentBuilder {
    let tarball_url = |version: &str| format!("https://registry.npmjs.org/@dprint/a/-/pkg-{}.tgz", version);
    let mut builder = TestEnvironmentBuilder::new();
    builder
      .add_remote_file_bytes(
        "https://registry.npmjs.org/@dprint/a",
        serde_json::json!({
          "dist-tags": { "latest": "1.1.0" },
          "versions": {
            "1.0.0": { "dist": { "tarball": tarball_url("1.0.0") } },
            "1.1.0": { "dist": { "tarball": tarball_url("1.1.0") } },
          },
          "time": {
            "1.0.0": "2099-01-01T00:00:00Z",
            "1.1.0": latest_time,
          }
        })
        .to_string()
        .into_bytes(),
      )
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          npm: Some(TestInfoFileNpm {
            name: "@dprint/a".to_string(),
            ..Default::default()
          }),
          ..wasm_plugin("a", "a", &["ts"])
        });
      })
      .write_file("/file.ts", "");
    builder
  }

  fn age_test_options(age: &str) -> GetInitConfigFileTextOptions {
    use std::str::FromStr;
    GetInitConfigFileTextOptions {
      non_interactive: true,
      minimum_dependency_age: Some(MinimumDependencyAgeArg::from_str(age).unwrap()),
      config_dir: None,
      options_loader: None,
    }
  }

  #[test]
  fn should_get_default_initialization_text() {
    // the typescript and json plugins are pre-selected because of these files
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ts", "")
      .write_file("/file.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "json": {
  },
  "excludes": [
    "**/something",
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_only_select_the_json_plugin_when_no_files_match() {
    // the config file that's about to be written is the only json file there is
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/readme.md", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_pre_select_plugin_by_file_name() {
    // the "final" plugin is matched via the Cargo.toml file name
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/Cargo.toml", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json",
    "**/something",
    "**other"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm",
    "https://plugins.dprint.dev/final-0.1.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_pre_select_process_plugin_by_extension() {
    // the process plugin is matched via its ".ps" file extension
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ps", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm",
    "https://plugins.dprint.dev/process-0.1.0.json@test-checksum"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_scan_files_in_nested_directories() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/src/nested/app.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // the typescript plugin is selected from the nested file
      assert!(text.contains("https://plugins.dprint.dev/typescript-0.17.2.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_scan_ignored_directories() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      // matching files only exist within ignored directories
      .write_file("/node_modules/dep/app.ts", "")
      .write_file("/.git/hooks/Cargo.toml", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // only the json plugin, which is selected for the config file itself
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_scan_files_in_hidden_directories() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      // hidden directories other than `.git` may hold files worth formatting
      .write_file("/.github/renovate.json", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("https://plugins.dprint.dev/json-0.2.3.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_not_scan_gitignored_files() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/.gitignore", "generated/\nCargo.toml\n")
      // matching files only exist where the gitignore excludes them
      .write_file("/generated/app.ts", "")
      .write_file("/Cargo.toml", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // only the json plugin, which is selected for the config file itself
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_match_extensions_case_insensitively() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/MAIN.TS", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert!(text.contains("https://plugins.dprint.dev/typescript-0.17.2.wasm"), "{text}");
      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_skip_prompt_when_non_interactive() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .write_file("/file.ts", "")
      .build();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(
        &environment,
        GetInitConfigFileTextOptions {
          non_interactive: true,
          ..Default::default()
        },
      )
      .await
      .unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "json": {
  },
  "excludes": [
    "**/something",
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      // no prompt should be shown when non-interactive
      assert_eq!(environment.take_stderr_messages(), Vec::<String>::new());
    });
  }

  #[test]
  fn should_get_initialization_text_when_can_access_url() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![0, 1, 2]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      // the json plugin is pre-selected (the config file is JSON), so it's
      // shown and written ahead of the ones that were selected from the rest
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "typescript": {
  },
  "excludes": [
    "**/*-asdf.json",
    "**/something",
    "**other"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm",
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/final-0.1.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_one_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    // the pre-selected json plugin is shown first, so this is it
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "json": {
  },
  "excludes": [
    "**/*-asdf.json"
  ],
  "plugins": [
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_no_plugins() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_process_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        for plugin in get_multi_plugins_config() {
          info.add_plugin(plugin);
        }
      })
      .build();
    environment.set_multi_selection_result(vec![3]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/process-0.1.0.json@test-checksum"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_cannot_access_url() {
    let environment = TestEnvironment::new();
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [
    "**/*-lock.json"
  ],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );
      let mut expected_messages = get_standard_logged_messages_no_plugin_selection();
      expected_messages.push(concat!(
        "There was a problem getting the latest plugin info, so the latest plugins may not be suggested. ",
        "Error: Error downloading https://plugins.dprint.dev/info.json - 404 Not Found"
      ));
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  #[test]
  fn should_get_initialization_text_when_selecting_other_option() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec!["test".to_string()],
          ..Default::default()
        });
      })
      .build();
    environment.set_selection_result(1);
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "typescript": {
  },
  "excludes": [
    "test"
  ],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm"
  ]
}
"#
      );

      assert_eq!(environment.take_stderr_messages(), get_standard_logged_messages());
    });
  }

  #[test]
  fn should_get_initialization_text_when_old_plugin_system() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.set_plugin_schema_version(999).add_plugin(TestInfoFilePlugin {
          name: "dprint-plugin-typescript".to_string(),
          version: "0.17.2".to_string(),
          url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
          config_key: Some("typescript".to_string()),
          file_extensions: vec!["ts".to_string()],
          config_excludes: vec!["asdf".to_string()],
          ..Default::default()
        });
      })
      .build();
    environment.set_multi_selection_result(vec![0]);
    environment.clone().run_in_runtime(async move {
      let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
      assert_eq!(
        text,
        r#"{
  "excludes": [
    "**/*-lock.json"
  ],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
      );
      let mut expected_messages = get_standard_logged_messages_no_plugin_selection();
      expected_messages.push(concat!(
        "You are using an old version of dprint so the latest plugins may not be suggested. ",
        "Consider upgrading to support new plugins. ",
        "Plugin system schema version is 4, latest is 999."
      ));
      assert_eq!(environment.take_stderr_messages(), expected_messages);
    });
  }

  const PREFERENCES_PATH: &str = "/config/dprint/init.json";

  /// Loads the same options for every plugin, counting how often it's asked.
  struct TestOptionsLoader {
    options: Vec<SettingsOption>,
    load_count: Rc<std::cell::Cell<usize>>,
  }

  #[dprint_core::async_runtime::async_trait(?Send)]
  impl InitPluginOptionsLoader for TestOptionsLoader {
    async fn load(&self, _plugin_entry: &str) -> Result<Vec<SettingsOption>> {
      self.load_count.set(self.load_count.get() + 1);
      Ok(self.options.clone())
    }
  }

  fn test_options_loader() -> (Rc<dyn InitPluginOptionsLoader>, Rc<std::cell::Cell<usize>>) {
    let load_count = Rc::new(std::cell::Cell::new(0));
    let loader = TestOptionsLoader {
      options: vec![
        SettingsOption {
          name: "lineWidth".to_string(),
          kind: SettingsOptionKind::Number,
          default_value: Some(serde_json::json!(120)),
          value: None,
          description: None,
        },
        SettingsOption {
          name: "useTabs".to_string(),
          kind: SettingsOptionKind::Bool,
          default_value: Some(serde_json::json!(false)),
          value: None,
          description: None,
        },
      ],
      load_count: load_count.clone(),
    };
    (Rc::new(loader), load_count)
  }

  fn options_with_loader(options_loader: Rc<dyn InitPluginOptionsLoader>) -> GetInitConfigFileTextOptions {
    GetInitConfigFileTextOptions {
      options_loader: Some(options_loader),
      ..Default::default()
    }
  }

  fn non_interactive_options() -> GetInitConfigFileTextOptions {
    GetInitConfigFileTextOptions {
      non_interactive: true,
      ..Default::default()
    }
  }

  /// Presses the keys in the prompts that get shown from here on. See
  /// `ScriptedPromptTerminal::new` for what a key is.
  fn press_keys(environment: &TestEnvironment, keys: &[&str]) {
    environment.set_prompt_keys(keys, None).unwrap();
  }

  /// What was on the screen after each key under the title and the key help,
  /// starting with what was shown before the first key.
  fn take_screens(environment: &TestEnvironment) -> Vec<Vec<String>> {
    environment
      .take_prompt_screens()
      .into_iter()
      .map(|screen| screen.lines.into_iter().skip(2).collect())
      .collect()
  }

  /// Creates the directory of [`PREFERENCES_PATH`], which macOS only uses
  /// when it's already there.
  fn create_preferences_dir(environment: &TestEnvironment) {
    environment.mk_dir_all("/config/dprint").unwrap();
  }

  fn write_preferences(environment: &TestEnvironment, preferred_plugins: Vec<PreferredPlugin>) {
    write_init_preferences(environment, &InitPreferences { preferred_plugins }).unwrap();
  }

  fn preferred(name: &str) -> PreferredPlugin {
    PreferredPlugin {
      name: name.to_string(),
      ..Default::default()
    }
  }

  #[test]
  fn should_pre_select_a_preferred_plugin_over_an_earlier_plugin_for_the_same_files() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    write_preferences(&environment, vec![preferred("b")]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, non_interactive_options()).await.unwrap();
        assert!(text.contains("b-1.0.0.wasm"), "{text}");
        assert!(!text.contains("a-1.0.0.wasm"), "{text}");

        // shown first and marked in the prompt
        get_init_config_file_text(&environment, Default::default()).await.unwrap();
        assert_eq!(environment.take_multi_selection_items(), vec!["[x] * b (.ts)", "[ ] a (.ts)"]);
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_pre_select_preferred_plugins_only_for_files_that_are_present() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut never_preselected = wasm_plugin("c", "c", &["ts"]);
        never_preselected.never_preselect = true;
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(wasm_plugin("b", "b", &["ts", "css"]))
          .add_plugin(never_preselected)
          .add_plugin(wasm_plugin("d", "d", &["md"]));
      })
      .write_file("/main.ts", "")
      .build();
    // `c` is preferred ahead of `b`, and there are no files for `d`
    write_preferences(&environment, vec![preferred("c"), preferred("b"), preferred("d")]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        get_init_config_file_text(&environment, Default::default()).await.unwrap();
        // both preferred plugins for .ts are selected because the user asked for
        // each of them, which includes the one that's otherwise never pre-selected
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] * c (.ts)", "[x] * b (.ts, .css)", "[ ] * d (.md)", "[ ] a (.ts)"]
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_pre_select_a_preferred_plugin_for_files_a_config_file_already_handles() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    write_preferences(&environment, vec![preferred("b")]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] a (.ts) \u{2014} already in config (locked)", "[ ] * b (.ts)"]
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_use_remembered_choices_for_a_plugin_a_config_file_already_has() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(wasm_plugin("a", "a", &["ts", "yaml"]))
          .add_plugin(wasm_plugin("b", "b", &["yaml"]));
      })
      .write_file("/main.ts", "")
      .write_file("/data.yaml", "")
      .build();
    write_preferences(
      &environment,
      vec![PreferredPlugin {
        name: "a".to_string(),
        excluded_file_extensions: vec!["yaml".to_string()],
        ..Default::default()
      }],
    );
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: None,
          },
        )
        .await
        .unwrap();
        // the config file's `a` formats .yaml no matter what was remembered
        assert_eq!(
          environment.take_multi_selection_items(),
          vec!["[x] * a (.ts, .yaml) \u{2014} already in config (locked)", "[ ] b (.yaml)"]
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_remember_the_preferred_plugins_in_the_order_they_are_left_in() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(wasm_plugin("b", "b", &["ts"]))
          .add_plugin(wasm_plugin("c", "c", &["md"]));
      })
      .write_file("/main.ts", "")
      .build();
    create_preferences_dir(&environment);
    // select `c` and then `b`, which goes ahead of `c` the way it does in the
    // info file until it's moved
    press_keys(&environment, &["down", "down", "space", "down", "down", "space", "shift+down", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        prompt_for_init_preferences(&environment, None).await.unwrap();
        let screens = take_screens(&environment);
        // nothing in the directory is highlighted or selected here
        assert_eq!(screens[0], vec!["> [ ] a (.ts)", "  [ ] b (.ts)", "  [ ] c (.md)"]);
        // a selected plugin goes up with the others that are
        assert_eq!(screens[3], vec!["> [x] c (.md)", "  [ ] a (.ts)", "  [ ] b (.ts)"]);
        assert_eq!(screens[6], vec!["> [x] b (.ts)", "  [x] c (.md)", "  [ ] a (.ts)"]);
        assert_eq!(screens[7], vec!["  [x] c (.md)", "> [x] b (.ts)", "  [ ] a (.ts)"]);
        assert!(!environment.path_exists("/dprint.json"));
        assert_eq!(
          read_init_preferences(&environment),
          InitPreferences {
            preferred_plugins: vec![preferred("c"), preferred("b")],
          }
        );
        let last_message = environment.take_stdout_messages().pop().unwrap();
        assert_eq!(last_message.replace('\\', "/"), "\nSaved the preferred plugins to /config/dprint/init.json");

        // the next time `b` is selected instead of `a`, and `c` has no files
        // here but keeps its place ahead of the plugins that aren't preferred
        press_keys(&environment, &["enter"]);
        get_init_config_file_text(&environment, Default::default()).await.unwrap();
        assert_eq!(
          take_screens(&environment)[0],
          vec!["> [x] b \u{2605} (.ts)", "  [ ] c \u{2605} (.md)", "  [ ] a (.ts)"]
        );
        // which leaves the order they're preferred in alone, even though
        // it's not the order they were shown in for this directory
        assert_eq!(
          read_init_preferences(&environment),
          InitPreferences {
            preferred_plugins: vec![preferred("c"), preferred("b")],
          }
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_forget_a_plugin_that_is_no_longer_preferred_and_keep_unknown_ones() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"])).add_plugin(wasm_plugin("b", "b", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    // `removed` isn't in the info file
    write_preferences(&environment, vec![preferred("removed"), preferred("b")]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        // it stays where it is, so nothing was changed
        press_keys(&environment, &["enter"]);
        prompt_for_init_preferences(&environment, None).await.unwrap();
        assert_eq!(environment.take_stdout_messages(), vec!["\nThe preferred plugins are the same as they were."]);
        // and it stays ahead of what's selected after it
        press_keys(&environment, &["down", "space", "enter"]);
        prompt_for_init_preferences(&environment, None).await.unwrap();
        assert_eq!(
          read_init_preferences(&environment),
          InitPreferences {
            preferred_plugins: vec![preferred("removed"), preferred("b"), preferred("a")],
          }
        );

        // `b` is listed first
        press_keys(&environment, &["space", "enter"]);
        prompt_for_init_preferences(&environment, None).await.unwrap();
        assert_eq!(
          read_init_preferences(&environment),
          InitPreferences {
            preferred_plugins: vec![preferred("removed"), preferred("a")],
          }
        );
        environment.take_stdout_messages();
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_save_preferences_when_non_interactive_or_they_were_not_changed() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts"]));
      })
      .write_file("/main.ts", "")
      .build();
    create_preferences_dir(&environment);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        get_init_config_file_text(&environment, non_interactive_options()).await.unwrap();
        get_init_config_file_text(&environment, Default::default()).await.unwrap();
        assert!(!environment.path_exists(PREFERENCES_PATH));

        // selecting and moving the config file's plugins isn't remembered
        press_keys(&environment, &["space", "shift+down", "enter"]);
        get_init_config_file_text(&environment, Default::default()).await.unwrap();
        assert!(!environment.path_exists(PREFERENCES_PATH));

        // and neither is what was done to the preferred plugins before cancelling
        press_keys(&environment, &["space", "esc"]);
        let err = prompt_for_init_preferences(&environment, None).await.unwrap_err();
        assert_eq!(err.to_string(), "Selection cancelled.");
        assert!(!environment.path_exists(PREFERENCES_PATH));

        // saving them unchanged doesn't write anything either
        press_keys(&environment, &["enter"]);
        prompt_for_init_preferences(&environment, None).await.unwrap();
        assert!(!environment.path_exists(PREFERENCES_PATH));
        assert_eq!(environment.take_stdout_messages(), vec!["\nThe preferred plugins are the same as they were."]);
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_write_negated_associations_for_the_files_the_user_said_to_leave_out() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("a", "a", &["ts", "yaml", "YML"]);
        plugin.file_names = Some(vec!["Dockerfile".to_string()]);
        plugin.default_config = Some(serde_json::json!({ "associations": ["**/*.custom"] }));
        info.add_plugin(plugin).add_plugin(wasm_plugin("b", "b", &["yaml"]));
      })
      .write_file("/main.ts", "")
      .write_file("/data.yaml", "")
      .build();
    // open `a` and uncheck everything apart from .ts
    press_keys(&environment, &["right", "down", "space", "down", "space", "down", "space", "left", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
        let screens = take_screens(&environment);
        // no loader was provided and nothing was chosen yet
        assert_eq!(
          screens[1],
          vec!["  Files to format:", "> [x] .ts", "  [x] .yaml", "  [x] .YML", "  [x] Dockerfile"]
        );
        // the selection isn't changed by what was done in the editor, so `b`
        // isn't selected for the .yaml file this time
        assert_eq!(screens[8], vec!["> [x] a (.ts) \u{2014} customized", "  [ ] b (.yaml)"]);
        assert_eq!(
          text,
          r#"{
  "a": {
    "associations": ["**/*.custom", "!**/*.yaml", "!**/*.YML", "!**/Dockerfile"]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/a-1.0.0.wasm"
  ]
}
"#
        );
        // what's chosen for the config file's plugins is never remembered
        assert!(!environment.path_exists(PREFERENCES_PATH));
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_remember_the_files_left_out_of_a_preferred_plugin() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info
          .add_plugin(wasm_plugin("a", "a", &["ts", "yaml"]))
          .add_plugin(wasm_plugin("b", "b", &["yaml"]));
      })
      .write_file("/main.ts", "")
      .write_file("/data.yaml", "")
      .build();
    // open `a` to uncheck .yaml, which selects it
    press_keys(&environment, &["right", "down", "space", "left", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        prompt_for_init_preferences(&environment, None).await.unwrap();
        let screens = take_screens(&environment);
        assert_eq!(screens[4], vec!["> [x] a (.ts) \u{2014} customized", "  [ ] b (.yaml)"]);
        environment.take_stdout_messages();
        assert_eq!(
          read_init_preferences(&environment),
          InitPreferences {
            preferred_plugins: vec![PreferredPlugin {
              name: "a".to_string(),
              excluded_file_extensions: vec!["yaml".to_string()],
              ..Default::default()
            }],
          }
        );

        // the next time `a` doesn't claim .yaml, so `b` is selected for it
        let text = get_init_config_file_text(&environment, non_interactive_options()).await.unwrap();
        assert_eq!(
          text,
          r#"{
  "a": {
    "associations": ["!**/*.yaml"]
  },
  "b": {
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/a-1.0.0.wasm",
    "https://plugins.dprint.dev/b-1.0.0.wasm"
  ]
}
"#
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_not_scaffold_a_config_item_for_files_the_user_said_to_leave_out() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(exec_info_plugin());
      })
      .write_file("/main.rs", "")
      .write_file("/main.go", "")
      .build();
    // open the plugin and uncheck .go
    press_keys(&environment, &["right", "down", "space", "left", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, Default::default()).await.unwrap();
        assert_eq!(take_screens(&environment)[1][..3], ["  Files to format:", "> [x] .rs", "  [x] .go"]);
        // the plugin doesn't match .go on its own, so there's nothing to negate
        assert!(text.contains("rustfmt"), "{text}");
        assert!(!text.contains("gofmt"), "{text}");
        assert!(!text.contains("associations"), "{text}");
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_write_and_remember_the_options_the_user_changed() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("a", "a", &["ts"]);
        plugin.default_config = Some(serde_json::json!({ "lineWidth": 100, "commands": ["x"] }));
        info.add_plugin(plugin);
      })
      .write_file("/main.ts", "")
      .build();
    let (options_loader, load_count) = test_options_loader();
    press_keys(
      &environment,
      &[
        "right",
        "wait",
        // change the line width to 80
        "down",
        "enter",
        "backspace",
        "backspace",
        "backspace",
        "\"80\"",
        "enter",
        // have this written even though it's the default
        "down",
        "tab",
        "left",
        "enter",
      ],
    );
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        prompt_for_init_preferences(&environment, Some(options_loader.clone())).await.unwrap();
        environment.take_stdout_messages();
        assert_eq!(load_count.get(), 1);
        assert_eq!(
          take_screens(&environment)[2],
          vec!["  Files to format:", "> [x] .ts", "  Options:", "    lineWidth: 120", "    useTabs: false"]
        );
        let expected_text = r#"{
  "a": {
    "lineWidth": 80,
    "commands": ["x"],
    "useTabs": false
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/a-1.0.0.wasm"
  ]
}
"#;
        let expected_preferences = InitPreferences {
          preferred_plugins: vec![PreferredPlugin {
            name: "a".to_string(),
            config: serde_json::json!({ "lineWidth": 80, "useTabs": false }).as_object().unwrap().clone(),
            ..Default::default()
          }],
        };
        assert_eq!(read_init_preferences(&environment), expected_preferences);

        // the next time it's written without anything being downloaded
        let text = get_init_config_file_text(&environment, non_interactive_options()).await.unwrap();
        assert_eq!(text, expected_text);
        assert_eq!(load_count.get(), 1);

        // and the editor starts out with what was remembered, where resetting
        // the line width to its default is only for this config file
        press_keys(&environment, &["right", "wait", "down", "delete", "left", "enter"]);
        let text = get_init_config_file_text(&environment, options_with_loader(options_loader)).await.unwrap();
        assert!(text.contains("\"lineWidth\": 100"), "{text}");
        let screens = take_screens(&environment);
        assert_eq!(
          screens[2],
          vec![
            "  Files to format:",
            "> [x] .ts",
            "  Options:",
            "    lineWidth: 80",
            "    useTabs: false (default, written)"
          ]
        );
        assert_eq!(screens[5], vec!["> [x] a \u{2605} (.ts) \u{2014} customized"]);
        assert_eq!(read_init_preferences(&environment), expected_preferences);
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_keep_the_options_when_the_user_leaves_before_they_load() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        info.add_plugin(wasm_plugin("a", "a", &["ts", "js"]));
      })
      .write_file("/main.ts", "")
      .build();
    write_preferences(
      &environment,
      vec![PreferredPlugin {
        name: "a".to_string(),
        config: serde_json::json!({ "lineWidth": 80 }).as_object().unwrap().clone(),
        ..Default::default()
      }],
    );
    let (options_loader, load_count) = test_options_loader();
    // the files can be changed while the options load
    press_keys(&environment, &["right", "down", "space", "left", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let text = get_init_config_file_text(&environment, options_with_loader(options_loader)).await.unwrap();
        // what was loading the options was dropped without being started
        assert_eq!(load_count.get(), 0);
        assert_eq!(
          take_screens(&environment)[3],
          vec![
            "  Files to format:",
            "  [x] .ts",
            "> [ ] .js",
            "  Options:",
            "    Downloading the plugin to get its options... (\u{2190} to cancel)"
          ]
        );
        assert_eq!(
          text,
          r#"{
  "a": {
    "lineWidth": 80,
    "associations": ["!**/*.js"]
  },
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/a-1.0.0.wasm"
  ]
}
"#
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn should_provide_what_was_chosen_for_a_plugin_added_to_an_existing_config_file() {
    let environment = TestEnvironmentBuilder::new()
      .with_info_file(|info| {
        let mut plugin = wasm_plugin("b", "b", &["json", "jsonc"]);
        // nothing is scaffolded when adding to a config file
        plugin.default_config = Some(serde_json::json!({ "scaffolded": true }));
        info
          .add_plugin(wasm_plugin("a", "a", &["ts"]))
          .add_plugin(plugin)
          .add_plugin(wasm_plugin("c", "c", &["md"]));
      })
      .write_file("/README.md", "")
      .build();
    let (options_loader, _) = test_options_loader();
    // open `b` to uncheck .jsonc and turn on `useTabs`
    press_keys(&environment, &["right", "wait", "down", "space", "down", "down", "space", "left", "enter"]);
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let plugins = get_init_plugins_to_add(
          &environment,
          GetInitPluginsToAddOptions {
            existing_plugin_names: Vec::from(["a".to_string()]),
            minimum_dependency_age: None,
            config_dir: None,
            options_loader: Some(options_loader),
          },
        )
        .await
        .unwrap();
        let InitPluginsToAdd::Plugins(plugins) = plugins else {
          unreachable!();
        };
        assert_eq!(
          plugins,
          vec![
            InitPluginToAdd {
              entry: "https://plugins.dprint.dev/b-1.0.0.wasm".to_string(),
              before: None,
              config: Some(("b".to_string(), serde_json::json!({ "useTabs": true, "associations": ["!**/*.jsonc"] }))),
            },
            InitPluginToAdd {
              entry: "https://plugins.dprint.dev/c-1.0.0.wasm".to_string(),
              before: None,
              config: None,
            },
          ]
        );
        environment.take_stderr_messages();
      }
    });
  }

  #[test]
  fn display_order_shows_preferred_plugins_first_in_each_group() {
    // 3 then 1 are preferred
    assert_eq!(display_order(&[true, true, false, true, false], &[], &[3, 1]), vec![3, 1, 0, 2, 4]);
    assert_eq!(display_order(&[true, false, false, true, false], &[], &[3, 1]), vec![3, 0, 1, 2, 4]);
    assert_eq!(display_order(&[true, false, false, false, false], &[3], &[3, 1]), vec![3, 0, 1, 2, 4]);
  }

  fn get_standard_logged_messages_no_plugin_selection() -> Vec<&'static str> {
    vec![]
  }

  fn get_standard_logged_messages() -> Vec<&'static str> {
    vec!["Select plugins:"]
  }

  fn get_multi_plugins_config() -> Vec<TestInfoFilePlugin> {
    vec![
      TestInfoFilePlugin {
        name: "dprint-plugin-typescript".to_string(),
        version: "0.17.2".to_string(),
        url: "https://plugins.dprint.dev/typescript-0.17.2.wasm".to_string(),
        config_key: Some("typescript".to_string()),
        file_extensions: vec!["ts".to_string(), "tsx".to_string()],
        config_excludes: vec!["**/something".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-jsonc".to_string(),
        version: "0.2.3".to_string(),
        url: "https://plugins.dprint.dev/json-0.2.3.wasm".to_string(),
        config_key: Some("json".to_string()),
        file_extensions: vec!["json".to_string()],
        config_excludes: vec!["**/*-asdf.json".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-plugin-final".to_string(),
        version: "0.1.2".to_string(),
        url: "https://plugins.dprint.dev/final-0.1.2.wasm".to_string(),
        file_names: Some(vec!["Cargo.toml".to_string()]),
        file_extensions: vec!["tsx".to_string(), "rs".to_string()],
        config_excludes: vec!["**/something".to_string(), "**other".to_string()],
        ..Default::default()
      },
      TestInfoFilePlugin {
        name: "dprint-process-plugin".to_string(),
        version: "0.1.0".to_string(),
        url: "https://plugins.dprint.dev/process-0.1.0.json".to_string(),
        file_extensions: vec!["ps".to_string()],
        config_excludes: vec![],
        checksum: Some("test-checksum".to_string()),
        ..Default::default()
      },
    ]
  }
}
