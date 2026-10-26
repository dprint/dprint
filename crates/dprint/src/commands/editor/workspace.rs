//! The editor service of schema version 6, which formats the files of a whole
//! workspace in one process by finding the config file of each file, the same
//! as the language server.

use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use dprint_core::plugins::HostFormatRequest;
use tokio_util::sync::CancellationToken;

use crate::arg_parser::CliArgs;
use crate::commands::scope_container::PluginsScopeContainer;
use crate::environment::Environment;
use crate::format::EnsureStableFormat;
use crate::paths::get_plugin_names_for_file_on_disk;
use crate::plugins::PluginResolver;
use crate::resolution::PluginsScope;
use crate::utils::Semaphore;

use super::EditorContext;
use super::get_editor_plugin_infos;
use super::messages::ConfigFileInfo;
use super::messages::ConfigInfoMessage;
use super::messages::ConfigInfoResponseMessage;
use super::messages::EditorMessageBody;
use super::messages::FileInfoMessage;
use super::messages::FileInfoResponseMessage;
use super::messages::FormatFileMessageBody;
use super::messages::FormatFileOptions;
use super::messages::FormatFileResult;
use super::messages::NotFormattedMessage;
use super::messages::NotFormattedReason;
use super::new_format_concurrency_limiter;
use super::send_error_response;
use super::send_response_body;
use super::start_reading_messages;

struct WorkspaceEditorService<TEnvironment: Environment> {
  environment: TEnvironment,
  context: EditorContext,
  scope_container: PluginsScopeContainer<TEnvironment>,
  concurrency_limiter: Rc<Semaphore>,
  ensure_stable_format: EnsureStableFormat,
}

pub async fn run_workspace_editor_service<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
  plugin_resolver: &Rc<PluginResolver<TEnvironment>>,
) -> Result<()> {
  let config_path = args.config.as_ref().map(|config| environment.cwd().join(config));
  let service = Rc::new(WorkspaceEditorService {
    environment: environment.clone(),
    context: EditorContext::new(environment),
    scope_container: PluginsScopeContainer::new(environment.clone(), plugin_resolver.clone(), config_path, args.remote_cache_mode()),
    concurrency_limiter: Rc::new(new_format_concurrency_limiter(environment)),
    ensure_stable_format: EnsureStableFormat::for_editor(environment),
  });
  let mut rx = start_reading_messages(environment);

  loop {
    let message = tokio::select! {
      message = rx.recv() => message,
      // there's nobody to respond to anymore
      _ = service.context.output_closed.cancelled() => None,
    };
    let Some(message) = message else {
      return Ok(());
    };
    let context = &service.context;
    match message.body {
      EditorMessageBody::Success(_) | EditorMessageBody::Error(_, _) => {}
      EditorMessageBody::Close => {
        send_response_body(context, EditorMessageBody::Success(message.id));
        return Ok(());
      }
      EditorMessageBody::IsAlive => {
        send_response_body(context, EditorMessageBody::Success(message.id));
      }
      EditorMessageBody::FileInfo(data) => {
        // these are handled at the same time, which the scope container
        // handles by only resolving each config file once
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = async {
            let message = serde_json::from_slice(&data).context("Error deserializing the file info message.")?;
            let response = service.get_file_info(message).await?;
            Ok::<_, anyhow::Error>(serde_json::to_vec(&response)?)
          }
          .await;
          match result {
            Ok(data) => send_response_body(&service.context, EditorMessageBody::FileInfoResponse(message.id, data)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::ConfigInfo(data) => {
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = async {
            let message = serde_json::from_slice(&data).context("Error deserializing the config info message.")?;
            let response = service.get_config_info(message).await?;
            Ok::<_, anyhow::Error>(serde_json::to_vec(&response)?)
          }
          .await;
          match result {
            Ok(data) => send_response_body(&service.context, EditorMessageBody::ConfigInfoResponse(message.id, data)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::FormatFile(body) => {
        // store this now so the format can be cancelled while it's waiting to be handled
        let token = Arc::new(CancellationToken::new());
        let token_storage_guard = context.cancellation_tokens.store_with_owned_guard(message.id, token.clone());
        let service = service.clone();
        dprint_core::async_runtime::spawn(async move {
          let result = service.format_file(body, &token).await;
          drop(token_storage_guard);
          // the editor isn't waiting on a cancelled format
          if token.is_cancelled() {
            return;
          }
          match result {
            Ok(result) => send_response_body(&service.context, EditorMessageBody::FormatFileResponse(message.id, result)),
            Err(err) => send_error_response(&service.context, message.id, err),
          }
        });
      }
      EditorMessageBody::CancelFormat(message_id) => {
        if let Some(token) = context.cancellation_tokens.get_cloned(message_id) {
          token.cancel();
        }
      }
      EditorMessageBody::CanFormat(_) | EditorMessageBody::Format(_) => {
        send_error_response(
          context,
          message.id,
          anyhow!("Message with kind {} isn't supported in schema version 6.", message.body.as_u32()),
        );
      }
      EditorMessageBody::CanFormatResponse(_, _)
      | EditorMessageBody::FormatResponse(_, _)
      | EditorMessageBody::FileInfoResponse(_, _)
      | EditorMessageBody::ConfigInfoResponse(_, _)
      | EditorMessageBody::FormatFileResponse(_, _) => {
        send_error_response(context, message.id, anyhow!("CLI cannot handle a message with kind {}.", message.body.as_u32()));
      }
      EditorMessageBody::Unknown(message_kind, _) => {
        send_error_response(context, message.id, anyhow!("Unknown message with kind: {}", message_kind));
      }
    }
  }
}

impl<TEnvironment: Environment> WorkspaceEditorService<TEnvironment> {
  async fn get_file_info(&self, message: FileInfoMessage) -> Result<FileInfoResponseMessage> {
    let Some(scope) = self.resolve_scope(&message.file_path, message.use_global_config).await? else {
      return Ok(FileInfoResponseMessage {
        config_file: None,
        can_format: false,
        reason: Some(NotFormattedReason::NoConfigFile),
      });
    };
    let file_path = self.canonicalize(message.file_path);
    let notebook_path = message.notebook_path.map(|path| self.canonicalize(path));
    // the editor service only receives a path, so an extensionless file's
    // shebang is resolved from the file on disk
    let reason = self.get_cant_format_reason(&scope, &file_path, notebook_path.as_deref(), None);
    Ok(FileInfoResponseMessage {
      config_file: get_config_file_info(&scope),
      can_format: reason.is_none(),
      reason,
    })
  }

  async fn get_config_info(&self, message: ConfigInfoMessage) -> Result<ConfigInfoResponseMessage> {
    let Some(scope) = self.resolve_scope(&message.file_path, message.use_global_config).await? else {
      return Ok(ConfigInfoResponseMessage {
        config_file: None,
        plugins: Vec::new(),
      });
    };
    Ok(ConfigInfoResponseMessage {
      config_file: get_config_file_info(&scope),
      plugins: get_editor_plugin_infos(&scope).await?,
    })
  }

  async fn format_file(&self, body: FormatFileMessageBody, token: &Arc<CancellationToken>) -> Result<FormatFileResult> {
    let options: FormatFileOptions = if body.options.is_empty() {
      Default::default()
    } else {
      serde_json::from_slice(&body.options).context("Error deserializing the format options.")?
    };
    let Some(scope) = self.resolve_scope(&body.file_path, options.use_global_config).await? else {
      return not_formatted(NotFormattedReason::NoConfigFile);
    };
    let file_path = self.canonicalize(body.file_path);
    let notebook_path = options.notebook_path.map(|path| self.canonicalize(path));
    if let Some(reason) = self.get_cant_format_reason(&scope, &file_path, notebook_path.as_deref(), Some(&body.file_bytes)) {
      return not_formatted(reason);
    }

    let _permit = self.concurrency_limiter.acquire().await;
    if token.is_cancelled() {
      // the response isn't sent for a cancelled format
      return not_formatted(NotFormattedReason::Unchanged);
    }
    let result = scope
      .format_stable(
        HostFormatRequest {
          file_path,
          file_bytes: body.file_bytes,
          range: body.range,
          override_config: options.override_config,
          token: token.clone(),
        },
        self.ensure_stable_format,
      )
      .await
      .map_err(|err| anyhow!("{}", dprint_core::plugins::error_to_string(&err)))?;
    match result {
      Some(text) => Ok(FormatFileResult::Formatted(text)),
      None => not_formatted(NotFormattedReason::Unchanged),
    }
  }

  /// Resolves the plugins of the config file used for the file, which is
  /// `None` when there isn't one.
  async fn resolve_scope(&self, file_path: &Path, use_global_config: bool) -> Result<Option<Rc<PluginsScope<TEnvironment>>>> {
    let Some(dir_path) = file_path.parent() else {
      bail!("Cannot format non-file path: {}", file_path.display());
    };
    let scope = self.scope_container.resolve_by_path(dir_path, use_global_config).await?;
    if let Some(scope) = &scope {
      scope.ensure_no_global_config_diagnostics()?;
    }
    Ok(scope)
  }

  /// Gets why the scope can't format the file, which is `None` when it can.
  fn get_cant_format_reason(
    &self,
    scope: &PluginsScope<TEnvironment>,
    file_path: &Path,
    notebook_path: Option<&Path>,
    file_bytes: Option<&[u8]>,
  ) -> Option<NotFormattedReason> {
    let can_format = match notebook_path {
      // the cli formats a notebook's cells when a plugin (the jupyter plugin) formats
      // the notebook, so only format a cell when the notebook would be formatted
      Some(notebook_path) => {
        !scope.plugin_name_maps.get_plugin_names_from_file_path(notebook_path).into_names().is_empty() && scope.can_format_for_editor(notebook_path, None)
      }
      None => scope.can_format_for_editor(file_path, file_bytes),
    };
    if !can_format {
      return Some(NotFormattedReason::NotMatched);
    }
    let has_plugin = match file_bytes {
      Some(file_bytes) => !scope
        .plugin_name_maps
        .get_plugin_names_from_file_path_and_bytes(file_path, file_bytes)
        .is_empty(),
      None => !get_plugin_names_for_file_on_disk(&scope.plugin_name_maps, file_path, &self.environment).is_empty(),
    };
    if has_plugin { None } else { Some(NotFormattedReason::NoPlugin) }
  }

  /// Canonicalizes the path so that it matches the canonicalized config paths
  /// (ex. for the includes and excludes).
  fn canonicalize(&self, path: PathBuf) -> PathBuf {
    self.environment.canonicalize_maybe_not_exists(&path).map(|p| p.into_path_buf()).unwrap_or(path)
  }
}

fn get_config_file_info<TEnvironment: Environment>(scope: &PluginsScope<TEnvironment>) -> Option<ConfigFileInfo> {
  let config = scope.config.as_ref()?;
  Some(ConfigFileInfo {
    path: config.source.display(),
    is_global: config.is_global,
  })
}

fn not_formatted(reason: NotFormattedReason) -> Result<FormatFileResult> {
  Ok(FormatFileResult::NotFormatted(serde_json::to_vec(&NotFormattedMessage { reason })?))
}

#[cfg(test)]
mod test {
  use std::io::Read;
  use std::io::Write;
  use std::path::Path;
  use std::path::PathBuf;
  use std::rc::Rc;

  use anyhow::Result;
  use anyhow::anyhow;
  use dprint_core::async_runtime::FutureExt;
  use dprint_core::async_runtime::LocalBoxFuture;
  use dprint_core::communication::IdGenerator;
  use dprint_core::communication::MessageReader;
  use dprint_core::communication::MessageWriter;
  use dprint_core::communication::RcIdStore;
  use dprint_core::communication::SingleThreadMessageWriter;
  use dprint_core::configuration::ConfigKeyMap;
  use pretty_assertions::assert_eq;
  use tokio::sync::oneshot;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::test_helpers::run_test_cli;

  use super::super::messages::ConfigFileInfo;
  use super::super::messages::ConfigInfoMessage;
  use super::super::messages::ConfigInfoResponseMessage;
  use super::super::messages::EditorMessage;
  use super::super::messages::EditorMessageBody;
  use super::super::messages::FileInfoMessage;
  use super::super::messages::FileInfoResponseMessage;
  use super::super::messages::FormatFileMessageBody;
  use super::super::messages::FormatFileOptions;
  use super::super::messages::FormatFileResult;
  use super::super::messages::NotFormattedMessage;
  use super::super::messages::NotFormattedReason;

  #[test]
  fn should_format_files_with_the_config_file_of_each() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "a" }"#);
      })
      .with_local_config("/b/dprint.json", |c| {
        c.add_remote_wasm_plugin()
          .add_config_section("test-plugin", r#"{ "ending": "b" }"#)
          .add_excludes("excluded.txt");
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.file_info(file_info("/a/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/a/dprint.json")),
            can_format: true,
            reason: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: true,
            reason: None,
          }
        );
        assert_eq!(
          client.file_info(file_info("/b/excluded.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: Some(local_config("/b/dprint.json")),
            can_format: false,
            reason: Some(NotFormattedReason::NotMatched),
          }
        );
        assert_eq!(
          client.file_info(file_info("/a/file.other")).await.unwrap().reason,
          Some(NotFormattedReason::NoPlugin)
        );
        assert_eq!(
          client.file_info(file_info("/c/file.txt")).await.unwrap(),
          FileInfoResponseMessage {
            config_file: None,
            can_format: false,
            reason: Some(NotFormattedReason::NoConfigFile),
          }
        );

        assert_eq!(client.format("/a/file.txt", "testing", None).await.unwrap(), formatted("testing_a"));
        assert_eq!(client.format("/b/file.txt", "testing", None).await.unwrap(), formatted("testing_b"));
        assert_eq!(
          client.format("/a/file.txt", "testing_a", None).await.unwrap(),
          not_formatted(NotFormattedReason::Unchanged)
        );
        assert_eq!(
          client.format("/b/excluded.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NotMatched)
        );
        assert_eq!(
          client.format("/c/file.txt", "testing", None).await.unwrap(),
          not_formatted(NotFormattedReason::NoConfigFile)
        );
        assert_eq!(
          client.format("/a/file.txt", "should_error", None).await.unwrap_err().to_string(),
          "Did error."
        );

        let config_info = client
          .config_info(ConfigInfoMessage {
            file_path: PathBuf::from("/b/dprint.json"),
            use_global_config: false,
          })
          .await
          .unwrap();
        assert_eq!(config_info.config_file, Some(local_config("/b/dprint.json")));
        assert_eq!(
          config_info.plugins.iter().map(|plugin| plugin.name.as_str()).collect::<Vec<_>>(),
          vec!["test-plugin"]
        );
        assert_eq!(config_info.plugins[0].file_extensions, vec!["txt"]);

        // the messages of schema version 5 aren't supported
        assert_eq!(
          client
            .request(EditorMessageBody::CanFormat(PathBuf::from("/a/file.txt")))
            .await
            .unwrap_err()
            .to_string(),
          "Message with kind 4 isn't supported in schema version 6."
        );
      }
      .boxed_local()
    });
    // the plugin is only compiled once for both config files
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
  }

  #[test]
  fn should_use_global_config_when_specified() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_global_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "global" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], |client| {
      async move {
        assert_eq!(
          client.file_info(file_info("/dir/file.txt")).await.unwrap().reason,
          Some(NotFormattedReason::NoConfigFile)
        );
        let info = client
          .file_info(FileInfoMessage {
            use_global_config: true,
            ..file_info("/dir/file.txt")
          })
          .await
          .unwrap();
        assert!(info.can_format);
        assert!(info.config_file.unwrap().is_global);

        let options = FormatFileOptions {
          use_global_config: true,
          ..Default::default()
        };
        assert_eq!(
          client.format("/dir/file.txt", "testing", Some(options.clone())).await.unwrap(),
          formatted("testing_global")
        );
        let mut override_config = ConfigKeyMap::new();
        override_config.insert("ending".to_string(), "override".into());
        assert_eq!(
          client
            .format("/dir/file.txt", "testing", Some(FormatFileOptions { override_config, ..options }))
            .await
            .unwrap(),
          formatted("testing_override")
        );
      }
      .boxed_local()
    });
  }

  #[test]
  fn should_use_the_config_file_after_it_changes() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_default_config(|c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "before" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &[], {
      let environment = environment.clone();
      move |client| {
        async move {
          assert_eq!(client.format("/file.txt", "testing", None).await.unwrap(), formatted("testing_before"));
          let config_text = environment.read_file("/dprint.json").unwrap().replace("before", "after");
          environment.write_file("/dprint.json", &config_text).unwrap();
          assert_eq!(client.format("/file.txt", "testing", None).await.unwrap(), formatted("testing_after"));
        }
        .boxed_local()
      }
    });
  }

  #[test]
  fn should_use_the_config_file_specified_with_config_arg() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_wasm_plugin()
      .with_local_config("/custom.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "specified" }"#);
      })
      .with_local_config("/a/dprint.json", |c| {
        c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "a" }"#);
      })
      .initialize()
      .build();

    run_session(&environment, &["--config", "/custom.json"], |client| {
      async move {
        assert_eq!(client.format("/a/file.txt", "testing", None).await.unwrap(), formatted("testing_specified"));
      }
      .boxed_local()
    });
    assert_eq!(environment.take_stderr_messages(), vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
  }

  #[test]
  fn should_not_start_schema_version_that_does_not_exist() {
    let environment = TestEnvironment::new();
    let err = run_test_cli(vec!["editor-service", "--parent-pid", "1", "--schema-version", "7"], &environment).unwrap_err();
    err.assert_exit_code(10);
    assert!(err.to_string().contains("7 is not in 5..=6"), "{err:#}");
  }

  #[derive(Debug, PartialEq)]
  enum FormatOutcome {
    Formatted(String),
    NotFormatted(NotFormattedReason),
  }

  fn formatted(text: &str) -> FormatOutcome {
    FormatOutcome::Formatted(text.to_string())
  }

  fn not_formatted(reason: NotFormattedReason) -> FormatOutcome {
    FormatOutcome::NotFormatted(reason)
  }

  fn file_info(file_path: &str) -> FileInfoMessage {
    FileInfoMessage {
      file_path: PathBuf::from(file_path),
      notebook_path: None,
      use_global_config: false,
    }
  }

  fn local_config(path: &str) -> ConfigFileInfo {
    ConfigFileInfo {
      path: path.to_string(),
      is_global: false,
    }
  }

  /// Runs the schema version 6 editor service with the client, which is shut
  /// down after the action.
  fn run_session(environment: &TestEnvironment, args: &[&str], action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + Send + 'static) {
    let stdin = environment.stdin_writer();
    let stdout = environment.stdout_reader();
    let result = std::thread::spawn(move || {
      TestEnvironment::new().run_in_runtime(async move {
        let client = Client::new(stdin, stdout);
        action(client.clone()).await;
        client.request(EditorMessageBody::Close).await.unwrap();
      });
    });
    // usually this would be the editor's process id, but this is ok for testing purposes
    let pid = std::process::id().to_string();
    let mut cli_args = vec!["editor-service", "--parent-pid", &pid, "--schema-version", "6"];
    cli_args.extend(args);
    run_test_cli(cli_args, environment).unwrap();
    result.join().unwrap();
  }

  /// An editor using schema version 6.
  #[derive(Clone)]
  struct Client {
    writer: Rc<SingleThreadMessageWriter<EditorMessage>>,
    id_generator: Rc<IdGenerator>,
    pending: RcIdStore<oneshot::Sender<EditorMessageBody>>,
  }

  impl Client {
    pub fn new(stdin: Box<dyn Write + Send>, stdout: Box<dyn Read + Send>) -> Self {
      let client = Client {
        writer: Rc::new(SingleThreadMessageWriter::for_stdin(MessageWriter::new(stdin))),
        id_generator: Default::default(),
        pending: Default::default(),
      };
      let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
      dprint_core::async_runtime::spawn_blocking(move || {
        let mut reader = MessageReader::new(stdout);
        while let Ok(message) = EditorMessage::read(&mut reader) {
          if tx.send(message).is_err() {
            break;
          }
        }
      });
      let pending = client.pending.clone();
      dprint_core::async_runtime::spawn(async move {
        while let Some(message) = rx.recv().await {
          let message_id = match &message.body {
            EditorMessageBody::Success(message_id)
            | EditorMessageBody::Error(message_id, _)
            | EditorMessageBody::FileInfoResponse(message_id, _)
            | EditorMessageBody::ConfigInfoResponse(message_id, _)
            | EditorMessageBody::FormatFileResponse(message_id, _) => *message_id,
            body => panic!("Unexpected message: {body:?}"),
          };
          if let Some(sender) = pending.take(message_id) {
            let _ = sender.send(message.body);
          }
        }
      });
      client
    }

    pub async fn file_info(&self, message: FileInfoMessage) -> Result<FileInfoResponseMessage> {
      match self.request(EditorMessageBody::FileInfo(serde_json::to_vec(&message)?)).await? {
        EditorMessageBody::FileInfoResponse(_, data) => Ok(serde_json::from_slice(&data)?),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    pub async fn config_info(&self, message: ConfigInfoMessage) -> Result<ConfigInfoResponseMessage> {
      match self.request(EditorMessageBody::ConfigInfo(serde_json::to_vec(&message)?)).await? {
        EditorMessageBody::ConfigInfoResponse(_, data) => Ok(serde_json::from_slice(&data)?),
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    pub async fn format(&self, file_path: impl AsRef<Path>, text: &str, options: Option<FormatFileOptions>) -> Result<FormatOutcome> {
      let body = EditorMessageBody::FormatFile(FormatFileMessageBody {
        file_path: file_path.as_ref().to_path_buf(),
        range: None,
        options: match options {
          Some(options) => serde_json::to_vec(&options)?,
          None => Vec::new(),
        },
        file_bytes: text.as_bytes().to_vec(),
      });
      match self.request(body).await? {
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::Formatted(text)) => Ok(FormatOutcome::Formatted(String::from_utf8(text)?)),
        EditorMessageBody::FormatFileResponse(_, FormatFileResult::NotFormatted(data)) => {
          Ok(FormatOutcome::NotFormatted(serde_json::from_slice::<NotFormattedMessage>(&data)?.reason))
        }
        body => panic!("Unexpected response: {body:?}"),
      }
    }

    /// Sends the message and gets the response, which is an error for an error response.
    pub async fn request(&self, body: EditorMessageBody) -> Result<EditorMessageBody> {
      let message_id = self.id_generator.next();
      let (tx, rx) = oneshot::channel();
      self.pending.store(message_id, tx);
      self.writer.send(EditorMessage { id: message_id, body })?;
      match rx.await? {
        EditorMessageBody::Error(_, data) => Err(anyhow!("{}", String::from_utf8_lossy(&data))),
        body => Ok(body),
      }
    }
  }
}
