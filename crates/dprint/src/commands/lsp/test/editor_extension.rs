//! Tests of an editor extension using the `dprint/*` methods.

use deno_tower_lsp::lsp_types::FileChangeType;
use deno_tower_lsp::lsp_types::FileEvent;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::commands::lsp::workspace::messages::ApprovalDecision;
use crate::commands::lsp::workspace::messages::ApprovalItemKind;
use crate::commands::lsp::workspace::messages::ApprovalKind;
use crate::commands::lsp::workspace::messages::ConfigFileInfo;
use crate::commands::lsp::workspace::messages::ConfigFileKind;
use crate::commands::lsp::workspace::messages::DocumentIdentifier;
use crate::commands::lsp::workspace::messages::FileInfoParams;
use crate::commands::lsp::workspace::messages::FilePlugin;
use crate::commands::lsp::workspace::messages::FormatFailedReason;
use crate::commands::lsp::workspace::messages::FormatStatus;
use crate::commands::lsp::workspace::messages::GlobalConfigUse;
use crate::commands::lsp::workspace::messages::NotFormatted;
use crate::commands::lsp::workspace::messages::PendingApproval;
use crate::commands::lsp::workspace::messages::PendingApprovalsParams;
use crate::commands::not_formatted_messages::NO_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NOT_MATCHED_MESSAGE;

#[test]
fn should_say_it_supports_the_dprint_methods() {
  let environment = TestEnvironmentBuilder::new().build();
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend(environment.clone());
    let run_test_task = dprint_core::async_runtime::spawn(async move {
      let result = initialize_backend(&backend, editor_extension_params()).await;
      // the version it uses
      assert_eq!(result.capabilities.experimental, Some(json!({ "dprint": { "version": 1 } })));
      // the text is sent when formatting, so the documents aren't synced
      assert_eq!(result.capabilities.text_document_sync, None);
      assert_eq!(result.capabilities.document_formatting_provider, None);
      backend.shutdown().await.unwrap();
    });
    try_join!(recv_task, run_test_task).unwrap();
    test_client.take_messages();
    assert!(test_client.take_registered_methods().is_empty());
  });

  // the client is handled like a language client when it doesn't support a
  // version the server does or its capability can't be read
  for experimental in [
    None,
    Some(json!({ "dprint": { "versions": [1, 2] } })),
    Some(json!({ "dprint": { "versions": [99] } })),
    Some(json!({ "dprint": { "version": 1 } })),
    Some(json!({ "dprint": null })),
  ] {
    let is_editor_extension = experimental == Some(json!({ "dprint": { "versions": [1, 2] } }));
    let environment = TestEnvironmentBuilder::new().build();
    environment.clone().run_in_runtime({
      let environment = environment.clone();
      async move {
        let (backend, recv_task, test_client) = setup_backend(environment.clone());
        let run_test_task = dprint_core::async_runtime::spawn(async move {
          let params = InitializeParams {
            capabilities: ClientCapabilities {
              experimental: experimental.clone(),
              ..Default::default()
            },
            ..Default::default()
          };
          let result = initialize_backend(&backend, params).await;
          if is_editor_extension {
            // the highest version both support
            assert_eq!(result.capabilities.experimental, Some(json!({ "dprint": { "version": 1 } })));
          } else {
            assert_eq!(result.capabilities.experimental, None, "{:?}", experimental);
            assert!(result.capabilities.text_document_sync.is_some());
          }
          backend.shutdown().await.unwrap();
        });
        try_join!(recv_task, run_test_task).unwrap();
        test_client.take_messages();
      }
    });
    // why it's handled like a language client is logged
    environment.take_stderr_messages();
  }
}

#[test]
fn should_send_and_receive_the_dprint_methods_as_json() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  environment.clone().run_in_runtime(async move {
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache, Default::default()));
    let (tx, rx) = mpsc::unbounded_channel();
    let recv_task = start_message_handler(&environment, &plugin_resolver, None, RemoteCacheMode::Use, Default::default(), rx);
    let (service, socket, pending) = with_dprint_methods(LspService::build(|client| {
      Backend::new(ClientWrapper::new(Arc::new(client)), environment.clone(), tx)
    }))
    .finish();
    let (client_writer, server_reader) = tokio::io::duplex(64 * 1024);
    let (server_writer, client_reader) = tokio::io::duplex(64 * 1024);
    let server_task = dprint_core::async_runtime::spawn(Server::new(server_reader, server_writer, socket, pending).serve(service));
    let mut connection = JsonRpcConnection {
      reader: tokio::io::BufReader::new(client_reader),
      writer: client_writer,
    };

    let response = connection
      .request(
        1,
        "initialize",
        json!({ "capabilities": { "experimental": { "dprint": { "versions": [1] } } } }),
      )
      .await;
    assert_eq!(response["result"]["capabilities"]["experimental"], json!({ "dprint": { "version": 1 } }));
    assert_eq!(response["result"]["capabilities"]["positionEncoding"], json!("utf-16"));
    connection.notify("initialized", json!({})).await;

    assert_eq!(
      connection
        .request(2, "dprint/format", json!({ "textDocument": { "uri": "file:///file.txt" }, "text": "text" }))
        .await,
      json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
          "kind": "formatted",
          "edits": [{
            "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 4 } },
            "newText": "_formatted",
          }],
        },
      })
    );
    assert_eq!(
      connection
        .request(3, "dprint/fileInfo", json!({ "textDocument": { "uri": "file:///file.txt" } }))
        .await["result"],
      json!({
        "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local" },
        "plugins": [{ "name": "test-plugin", "version": "0.2.0", "configKey": "test-plugin" }],
        "status": { "kind": "ready" },
      })
    );
    assert_eq!(
      connection
        .request(30, "dprint/fileInfo", json!({ "textDocument": { "uri": "git:/file.txt" } }))
        .await["result"]["status"]["reason"],
      json!("unsupportedDocument")
    );
    assert_eq!(
      connection.request(4, "dprint/configInfo", json!({ "uri": "file:///dprint.json" })).await["result"],
      json!({
        "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local" },
        "plugins": [{
          "name": "test-plugin",
          "version": "0.2.0",
          "configKey": "test-plugin",
          "fileExtensions": ["txt"],
          "fileNames": [],
          "additive": false,
          "supportsRangeFormatting": null,
          "helpUrl": "https://dprint.dev/plugins/test",
          "updateUrl": "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        }],
        "formatsOtherFiles": false,
        "filePatterns": ["**/*.[tT][xX][tT]"],
        "watchUris": [],
        "status": { "kind": "ready" },
      })
    );
    assert_eq!(
      connection.request(5, "dprint/approve", json!({ "id": "unknown", "decision": "allow" })).await["error"]["code"],
      json!(-32801)
    );
    assert_eq!(
      connection
        .request(
          6,
          "dprint/format",
          json!({ "textDocument": { "uri": "file:///file.txt" }, "text": "text", "overrideConfig": { "lineWidth": 80 } })
        )
        .await["error"]["code"],
      json!(-32602)
    );

    assert_eq!(
      connection.request(8, "dprint/pendingApprovals", json!({})).await["result"],
      json!({ "approvals": [] })
    );
    assert_eq!(connection.request(9, "dprint/reset", json!({})).await["result"], json!(null));

    assert_eq!(connection.request(7, "shutdown", json!(null)).await["result"], json!(null));
    connection.notify("exit", json!(null)).await;
    drop(connection);
    let (_, shutdown_received) = try_join!(server_task, recv_task).unwrap();
    assert!(shutdown_received);
  });
}

/// The client side of a connection to the language server, which sends and
/// receives the JSON-RPC messages as an editor does.
struct JsonRpcConnection {
  reader: tokio::io::BufReader<tokio::io::DuplexStream>,
  writer: tokio::io::DuplexStream,
}

impl JsonRpcConnection {
  /// Sends the request and gets its response, which ignores the messages the
  /// server sends before it (ex. what's logged). A request without params has
  /// `null` for them.
  async fn request(&mut self, id: u32, method: &str, params: serde_json::Value) -> serde_json::Value {
    self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method }), params).await;
    loop {
      let message = self.receive().await;
      if message.get("method").is_none() && message["id"] == json!(id) {
        return message;
      }
    }
  }

  async fn notify(&mut self, method: &str, params: serde_json::Value) {
    self.send(json!({ "jsonrpc": "2.0", "method": method }), params).await;
  }

  async fn send(&mut self, mut message: serde_json::Value, params: serde_json::Value) {
    use tokio::io::AsyncWriteExt;
    if !params.is_null() {
      message["params"] = params;
    }
    let body = message.to_string();
    let text = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    self.writer.write_all(text.as_bytes()).await.unwrap();
  }

  async fn receive(&mut self) -> serde_json::Value {
    use tokio::io::AsyncBufReadExt;
    use tokio::io::AsyncReadExt;
    let mut content_length = 0;
    loop {
      let mut line = String::new();
      self.reader.read_line(&mut line).await.unwrap();
      let line = line.trim_end();
      if line.is_empty() {
        break;
      }
      if let Some(value) = line.strip_prefix("Content-Length: ") {
        content_length = value.parse().unwrap();
      }
    }
    let mut body = vec![0; content_length];
    self.reader.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
  }
}

#[test]
fn should_format_files_with_the_config_file_of_each() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "a" }"#);
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_config_section("test-plugin", r#"{ "ending": "b", "associations": ["**/*.other"] }"#)
        .add_excludes("excluded.txt");
    })
    .initialize()
    .build();

  run_session(&environment, |client| {
    async move {
      assert_eq!(
        client.file_info("/a/file.txt").await,
        FileInfoResult {
          config_file: Some(local_config("/a/dprint.json")),
          plugins: vec![test_plugin()],
          status: FormatStatus::Ready,
        }
      );
      assert_eq!(
        client.file_info("/b/excluded.txt").await,
        FileInfoResult {
          config_file: Some(local_config("/b/dprint.json")),
          plugins: Vec::new(),
          status: not_formatted_status(NotFormattedReason::NotMatched, Some(NOT_MATCHED_MESSAGE)),
        }
      );
      assert_eq!(client.file_info("/a/file.other").await.status.reason(), Some(NotFormattedReason::NoPlugin));
      assert_eq!(
        client.file_info("/c/file.txt").await,
        FileInfoResult {
          config_file: None,
          plugins: Vec::new(),
          status: not_formatted_status(
            NotFormattedReason::NoConfigFile,
            Some("No dprint configuration file found. Run \"dprint init\" in your project to create one."),
          ),
        }
      );

      assert_eq!(client.format("/a/file.txt", "testing").await, formatted("testing", "testing_a"));
      assert_eq!(client.format("/b/file.txt", "testing").await, formatted("testing", "testing_b"));
      // already formatted
      assert_eq!(client.format("/a/file.txt", "testing_a").await, FormatResult::Formatted { edits: Vec::new() });
      assert_eq!(
        client.format("/b/excluded.txt", "testing").await,
        not_formatted(NotFormattedReason::NotMatched, Some(NOT_MATCHED_MESSAGE))
      );
      assert_eq!(
        client.format("/a/file.other", "testing").await,
        not_formatted(NotFormattedReason::NoPlugin, Some(NO_PLUGIN_MESSAGE))
      );
      assert_eq!(
        client.format("/a/file.txt", "should_error").await,
        FormatResult::Failed {
          reason: FormatFailedReason::FormatError,
          message: "Did error.".to_string(),
          plugin: Some(test_plugin()),
        }
      );
      let FormatResult::Failed { reason, message, .. } = client.format("/a/file.txt", "should_panic").await else {
        panic!("expected it to fail");
      };
      assert_eq!(reason, FormatFailedReason::PluginError);
      assert!(message.contains("panicked"), "{}", message);
      // the plugin is started again
      assert_eq!(client.format("/a/file.txt", "testing").await, formatted("testing", "testing_a"));

      let config_info = client.config_info("/b/dprint.json").await;
      assert_eq!(config_info.config_file, Some(local_config("/b/dprint.json")));
      let plugins = config_info.plugins.unwrap();
      assert_eq!(plugins.iter().map(|plugin| plugin.name.as_str()).collect::<Vec<_>>(), vec!["test-plugin"]);
      assert_eq!(plugins[0].file_extensions, vec!["txt"]);
      // the associations format other files, which the file info says
      assert!(config_info.formats_other_files);
      assert!(!client.config_info("/a/dprint.json").await.formats_other_files);
      assert_eq!(config_info.status, FormatStatus::Ready);
    }
    .boxed_local()
  });
}

#[test]
fn should_not_notify_an_editor_extension_about_a_file_without_a_config_file() {
  let environment = TestEnvironmentBuilder::new().build();
  ensure_global_config_dir(&environment);
  run_session(&environment, |client| {
    async move {
      assert_eq!(
        client.format("/file.txt", "text").await,
        not_formatted(
          NotFormattedReason::NoConfigFile,
          Some("No dprint configuration file found. Run \"dprint init\" in your project to create one."),
        )
      );
      // the editor extension notifies the user itself
      assert!(client.test_client.take_shown_messages().is_empty());
    }
    .boxed_local()
  });
}

#[test]
fn should_use_the_global_config_file_when_asked() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_global_config(|c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let result = client.format("/file.txt", "text").await;
      let FormatResult::NotFormatted(NotFormatted { reason, message, .. }) = result else {
        panic!("expected it to not be formatted");
      };
      assert_eq!(reason, NotFormattedReason::GlobalConfigNotAllowed);
      assert!(message.unwrap().contains("enable the \"useGlobalConfig\" setting of the dprint extension"));
      let mut params = client.format_params("/file.txt", "text");
      params.document.global_config = Some(GlobalConfigUse::WhenNoConfigFile);
      assert_eq!(client.format_with(params).await, formatted("text", "text_formatted"));
      let info = client
        .backend
        .dprint_file_info(
          FileInfoParams {
            document: DocumentParams {
              global_config: Some(GlobalConfigUse::WhenNoConfigFile),
              ..document_params("/file.txt")
            },
            text: None,
          },
          CancellationToken::new(),
        )
        .await
        .unwrap();
      assert_eq!(info.status, FormatStatus::Ready);
      assert_eq!(info.config_file.unwrap().kind, ConfigFileKind::Global);
      // the setting
      client
        .backend
        .did_change_configuration(DidChangeConfigurationParams {
          settings: json!({ "useGlobalConfig": true }),
        })
        .await;
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
      // a document can not use it regardless of the setting
      let mut params = client.format_params("/file.txt", "text");
      params.document.global_config = Some(GlobalConfigUse::Never);
      assert_eq!(
        client.format_with(params).await.not_formatted_reason(),
        Some(NotFormattedReason::GlobalConfigNotAllowed)
      );
    }
    .boxed_local()
  });
}

#[test]
fn should_provide_the_directory_a_config_file_is_for() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let config_file = client.config_info("/a/file.txt").await.config_file.unwrap();
      assert_eq!(config_file, local_config("/a/.config/dprint.json"));
      assert_eq!(config_file.dir_uri, Some(uri("/a")));
    }
    .boxed_local()
  });
}

#[test]
fn should_only_override_the_indentation_config() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let mut params = client.format_params("/file.txt", "text");
      params.override_config = ConfigKeyMap::from([
        ("indentWidth".to_string(), ConfigKeyValue::from_i32(4)),
        ("useTabs".to_string(), ConfigKeyValue::from_bool(true)),
      ]);
      assert_eq!(client.format_with(params).await, formatted("text", "text_formatted"));

      // other config could have a plugin do something that wasn't approved
      let mut params = client.format_params("/file.txt", "text");
      params.override_config = ConfigKeyMap::from([("ending".to_string(), ConfigKeyValue::from_str("other"))]);
      let err = client.backend.dprint_format(params, CancellationToken::new()).await.unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
    }
    .boxed_local()
  });
}

#[test]
fn should_format_ranges_with_utf16_positions() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      // the positions after the emoji are in utf-16 code units
      let text = "\u{1F995}text";
      assert_eq!(
        client.format("/file.txt", text).await,
        FormatResult::Formatted {
          edits: vec![TextEdit {
            range: Range::new(Position::new(0, 6), Position::new(0, 6)),
            new_text: "_formatted".to_string(),
          }],
        }
      );
      let mut params = client.format_params("/file.txt", "a\nb\nc\n");
      params.ranges = Some(vec![
        Range::new(Position::new(0, 0), Position::new(0, 1)),
        Range::new(Position::new(2, 0), Position::new(2, 1)),
      ]);
      let FormatResult::Formatted { edits } = client.format_with(params).await else {
        panic!("expected it to be formatted");
      };
      assert!(!edits.is_empty());
    }
    .boxed_local()
  });
}

#[test]
fn should_format_notebook_cells_and_untitled_documents_by_their_language() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let language = DocumentLanguage {
        id: "plaintext".to_string(),
        file_extensions: vec![".txt".to_string()],
        file_names: Vec::new(),
      };
      let mut params = client.format_params("untitled:Untitled-1", "text");
      params.document.language = Some(language.clone());
      assert_eq!(client.format_with(params).await, formatted("text", "text_formatted"));
      // the language is needed to know what to format it as
      let params = client.format_params("untitled:Untitled-1", "text");
      assert_eq!(
        client.format_with(params).await.not_formatted_reason(),
        Some(NotFormattedReason::LanguageRequired)
      );
      // a document that isn't a file
      let params = client.format_params("git:/file.txt", "text");
      assert_eq!(
        client.format_with(params).await.not_formatted_reason(),
        Some(NotFormattedReason::UnsupportedDocument)
      );
      // a notebook cell is only formatted when the notebook would be, which no plugin formats
      let mut params = client.format_params("vscode-notebook-cell:/notebook.ipynb#W0", "text");
      params.document.language = Some(language);
      params.document.notebook_document = Some(DocumentIdentifier { uri: uri("/notebook.ipynb") });
      let FormatResult::NotFormatted(NotFormatted { reason, .. }) = client.format_with(params).await else {
        panic!("expected it to not be formatted");
      };
      assert_eq!(reason, NotFormattedReason::NoPlugin);
    }
    .boxed_local()
  });
}

#[test]
fn should_send_what_is_logged_and_the_progress_to_an_editor_extension() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build(); // not initialized, so the plugin is compiled
  run_session(&environment, |client| {
    async move {
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
      let progress = client.test_client.progress.lock().drain(..).map(|(_, progress)| progress).collect::<Vec<_>>();
      let titles = progress
        .iter()
        .filter_map(|progress| match progress {
          WorkDoneProgress::Begin(begin) => Some(begin.title.clone()),
          _ => None,
        })
        .collect::<Vec<_>>();
      assert_eq!(titles, vec!["Compiling https://plugins.dprint.dev/test-plugin.wasm"]);
      assert!(matches!(progress.last(), Some(WorkDoneProgress::End(_))));
    }
    .boxed_local()
  });
  // it went to the editor instead
  assert!(environment.take_stderr_messages().is_empty());
}

#[test]
fn should_report_progress_on_the_token_of_a_request() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build(); // not initialized, so the plugin is compiled
  run_session(&environment, |client| {
    async move {
      let token = NumberOrString::String("format-1".to_string());
      let mut params = client.format_params("/file.txt", "text");
      params.document.work_done_token = Some(token.clone());
      assert_eq!(client.format_with(params).await, formatted("text", "text_formatted"));
      // the progress is ended once the request is handled, which is after the response
      let mut request_progress = Vec::new();
      for _ in 0..200 {
        request_progress.extend(
          client
            .test_client
            .progress
            .lock()
            .drain(..)
            .filter(|(progress_token, _)| *progress_token == token)
            .map(|(_, progress)| progress),
        );
        if matches!(request_progress.last(), Some(WorkDoneProgress::End(_))) {
          break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
      }
      let WorkDoneProgress::Begin(begin) = &request_progress[0] else {
        panic!("expected it to begin: {:?}", request_progress);
      };
      assert_eq!(begin.title, "Compiling https://plugins.dprint.dev/test-plugin.wasm");
      assert!(matches!(request_progress.last(), Some(WorkDoneProgress::End(_))));
    }
    .boxed_local()
  });
}

#[test]
fn should_resolve_a_config_file_again_when_a_file_it_extends_changed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("extends", r#""./base.json""#);
    })
    .initialize()
    .build();
  environment.write_file("/base.json", r#"{ "test-plugin": { "ending": "base" } }"#).unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let config_info = client.config_info("/dprint.json").await;
        assert_eq!(config_info.watch_uris, vec![uri("/base.json")]);
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_base"));

        environment.write_file("/base.json", r#"{ "test-plugin": { "ending": "changed" } }"#).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/base.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/dprint.json")]);
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_changed"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_resolve_a_config_file_again_when_a_broken_file_it_extends_is_fixed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("extends", r#""./base.json""#);
    })
    .initialize()
    .build();
  environment.write_file("/base.json", r#"{ "test-plugin": { "ending": "base" } }"#).unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let did_change_base = || DidChangeWatchedFilesParams {
          changes: vec![FileEvent {
            uri: uri("/base.json"),
            typ: FileChangeType::CHANGED,
          }],
        };
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::Ready);

        // breaking it still has it watched
        environment.write_file("/base.json", "{").unwrap();
        client.backend.did_change_watched_files(did_change_base()).await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/dprint.json")]);
        let config_info = client.config_info("/dprint.json").await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ConfigError));
        assert_eq!(config_info.watch_uris, vec![uri("/base.json")]);

        // fixing it is noticed
        environment.write_file("/base.json", r#"{ "test-plugin": { "ending": "fixed" } }"#).unwrap();
        client.backend.did_change_watched_files(did_change_base()).await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/dprint.json")]);
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::Ready);
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_fixed"));
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_associations_relative_to_the_directory_of_the_config_file() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section(
        "test-plugin",
        r#"{ "associations": ["*.foo", "/src/*.bar", "./b/*.baz", "../c/*.qux", "!*.foo"] }"#,
      );
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      assert_eq!(
        client.config_info("/a/dprint.json").await.file_patterns,
        vec!["**/*.[tT][xX][tT]", "**/*.foo", "src/*.bar", "b/*.baz"]
      );
    }
    .boxed_local()
  });
}

#[test]
fn should_get_the_plugins_of_an_unsaved_file_by_its_shebang() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_config_section("shebangs", r##"{ "#!/usr/bin/env test": "txt" }"##);
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      // the file isn't on disk
      assert_ne!(client.file_info("/script").await.status, FormatStatus::Ready);
      let file_info = client
        .backend
        .dprint_file_info(
          FileInfoParams {
            document: document_params("/script"),
            text: Some("#!/usr/bin/env test\ntext".to_string()),
          },
          CancellationToken::new(),
        )
        .await
        .unwrap();
      assert_eq!(file_info.status, FormatStatus::Ready);
      assert_eq!(file_info.plugins, vec![test_plugin()]);
    }
    .boxed_local()
  });
}

#[test]
fn should_publish_the_diagnostics_of_config_files() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.config_info("/dprint.json").await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].0, uri("/dprint.json"));
        assert_eq!(diagnostics[0].1.len(), 1);
        assert_eq!(diagnostics[0].1[0].message, "Unknown property in configuration");
        // where the property is
        assert_eq!(diagnostics[0].1[0].range, Range::new(Position::new(1, 2), Position::new(1, 19)));

        // they're cleared once fixed
        environment
          .write_file("/dprint.json", r#"{ "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"] }"#)
          .unwrap();
        client.config_info("/dprint.json").await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics, vec![(uri("/dprint.json"), Vec::new())]);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_resolve_a_config_file_again_when_the_editor_says_it_changed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/other/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.config_info("/dprint.json").await;
        assert!(client.test_client.published_diagnostics.lock().drain(..).next().is_none());
        let did_change = |path: &str, change_type| DidChangeWatchedFilesParams {
          changes: vec![FileEvent {
            uri: uri(path),
            typ: change_type,
          }],
        };

        // the diagnostics are published without another request
        environment.write_file("/dprint.json", "{").unwrap();
        client
          .backend
          .did_change_watched_files(did_change("/dprint.json", FileChangeType::CHANGED))
          .await;
        let diagnostics = client.wait_for_published_diagnostics().await;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].0, uri("/dprint.json"));
        assert_eq!(diagnostics[0].1.len(), 1);
        assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
        client.test_client.take_shown_messages();

        // a config file that wasn't used isn't resolved, which would set up its plugins
        environment.write_file("/other/dprint.json", "{").unwrap();
        client
          .backend
          .did_change_watched_files(did_change("/other/dprint.json", FileChangeType::CHANGED))
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/other/dprint.json")]);
        assert!(client.test_client.published_diagnostics.lock().is_empty());
        // and neither is one in node_modules
        client
          .backend
          .did_change_watched_files(did_change("/node_modules/package/dprint.json", FileChangeType::CREATED))
          .await;

        // the diagnostics are cleared once it's deleted
        environment.remove_file("/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(did_change("/dprint.json", FileChangeType::DELETED))
          .await;
        assert_eq!(client.wait_for_published_diagnostics().await, vec![(uri("/dprint.json"), Vec::new())]);
        assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_plugins_known_before_when_a_config_file_has_an_error() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let plugins = client.config_info("/dprint.json").await.plugins;
        assert_eq!(plugins.as_ref().map(|plugins| plugins.len()), Some(1));
        // a typo doesn't change what the editor formats with it
        environment.write_file("/dprint.json", "{").unwrap();
        let config_info = client.config_info("/dprint.json").await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ConfigError));
        assert_eq!(config_info.plugins, plugins);
        // the editor extension shows the error, which the server doesn't
        assert!(config_info.status.message().is_some());
        assert!(client.test_client.take_shown_messages().is_empty());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_plugins_known_before_in_another_session_when_they_need_to_be_approved() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "associations": ["**/*.other"] }"#);
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      let config_info = client.config_info("/dprint.json").await;
      assert_eq!(config_info.status, FormatStatus::Ready);
      assert_eq!(config_info.plugins.unwrap().len(), 2);
    }
    .boxed_local()
  });
  // they were only allowed for the session, so a new session needs them approved
  run_session(&environment, |client| {
    async move {
      let config_info = client.config_info("/dprint.json").await;
      assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
      let plugins = config_info.plugins.unwrap();
      assert_eq!(plugins.len(), 2);
      assert_eq!(plugins[1].config_key, "testProcessPlugin");
      assert_eq!(plugins[1].file_extensions, vec!["txt_ps".to_string()]);
      // the associations are known without the plugins
      assert!(config_info.formats_other_files);
    }
    .boxed_local()
  });
}

#[test]
fn should_approve_downloading_from_domains_that_are_not_trusted() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // the plugins aren't known, so the editor doesn't format with it until it's approved
        let config_info = client.config_info("/dprint.json").await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
        assert_eq!(config_info.plugins, None);
        let approvals = config_info.status.approvals();
        assert_eq!(approvals.len(), 1);
        let approval = approvals[0].clone();
        assert_eq!(approval.kind, ApprovalKind::Download);
        assert_eq!(specifiers(&approval), vec![UNTRUSTED_PLUGIN_URL]);
        assert_eq!(approval.items[0].kind, Some(ApprovalItemKind::Plugin));
        // a download is only allowed for the session
        assert_eq!(approval.decisions, vec![ApprovalDecision::Allow, ApprovalDecision::Deny]);
        assert!(!approval.denied);
        assert!(
          approval
            .message
            .starts_with("dprint needs to download https://example.com/test-plugin.wasm from domains that aren't trusted.")
        );
        assert_eq!(
          client.format("/file.txt", "text").await,
          FormatResult::NotFormatted(NotFormatted {
            reason: NotFormattedReason::ApprovalRequired,
            message: Some(approval.message.clone()),
            approvals: vec![approval.clone()],
          })
        );
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 0);
        // the server doesn't ask the user itself
        assert!(client.test_client.take_shown_messages().is_empty());

        // a decision it doesn't have
        let err = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: approval.id.clone(),
              decision: ApprovalDecision::AlwaysAllow,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);

        // denying it returns the approval that's still needed
        let denied = client.approve(&approval.id, ApprovalDecision::Deny).await;
        assert!(denied[0].denied);
        assert!(client.config_info("/dprint.json").await.status.approvals()[0].denied);
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 0);

        // it's pending until it's allowed
        assert_eq!(client.pending_approvals().await.len(), 1);

        // the user can still allow it (ex. with a command)
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(client.pending_approvals().await, Vec::new());
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 1);
        // the editor is told the config file formats files now
        assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::Ready);

        // an approval that isn't pending anymore says what needs to be approved now
        let err = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: approval.id.clone(),
              decision: ApprovalDecision::Allow,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ContentModified);
        assert_eq!(err.data, Some(json!({ "approvals": [] })));

        // an approval that isn't pending anymore says what's needed changed
        let err = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: "unknown".to_string(),
              decision: ApprovalDecision::Allow,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ContentModified);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_approve_what_an_allowed_config_file_extends() {
  const UNTRUSTED_CONFIG_URL: &str = "https://example.com/dprint.json";
  const UNTRUSTED_BASE_CONFIG_URL: &str = "https://other.example.com/base.json";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_config_section("extends", &format!("\"{}\"", UNTRUSTED_CONFIG_URL));
    })
    .build();
  environment.add_remote_file(
    UNTRUSTED_CONFIG_URL,
    br#"{ "extends": "https://other.example.com/base.json", "test-plugin": { "ending": "extended" } }"#,
  );
  environment.add_remote_file(UNTRUSTED_BASE_CONFIG_URL, br#"{ "plugins": ["https://plugins.dprint.dev/test-plugin.wasm"] }"#);
  run_session(&environment, |client| {
    async move {
      let approval = client.format("/file.txt", "text").await.approvals()[0].clone();
      assert_eq!(specifiers(&approval), vec![UNTRUSTED_CONFIG_URL]);
      assert_eq!(approval.items[0].kind, Some(ApprovalItemKind::Config));
      // allowing it only allows what was shown, so what it extends needs to be approved too
      let approval = client.approve(&approval.id, ApprovalDecision::Allow).await[0].clone();
      assert_eq!(approval.kind, ApprovalKind::Download);
      assert_eq!(specifiers(&approval), vec![UNTRUSTED_BASE_CONFIG_URL]);
      assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      // the editor is told the config file formats files now
      assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_extended"));
    }
    .boxed_local()
  });
}

#[test]
fn should_approve_running_process_plugins() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build(); // not initialized, so the plugins aren't set up
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let file_info = client.file_info("/a/file.txt").await;
        assert_eq!(file_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
        let approval = file_info.status.approvals()[0].clone();
        assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
        assert_eq!(approval.config_file, Some(uri("/a/dprint.json")));
        assert_eq!(approval.decisions.len(), 4);
        assert_eq!(approval.items.len(), 1);
        assert!(approval.items[0].specifier.starts_with("https://plugins.dprint.dev/test-process.json@"));
        // what it is and its configuration isn't known until it's set up
        assert_eq!(approval.items[0].config_key, None);
        assert_eq!(approval.items[0].configuration, None);
        assert!(approval.message.contains(") weren't allowed to run."), "{}", approval.message);
        // none of the plugins are set up until then
        assert_eq!(client.config_info("/a/dprint.json").await.plugins, None);

        // denying it is remembered for the session, which a format says
        let denied = client.approve(&approval.id, ApprovalDecision::Deny).await;
        assert!(denied[0].denied);
        assert_eq!(denied[0].previous_decision, Some(ApprovalDecision::Deny));
        let result = client.format("/a/file.txt_ps", "text").await;
        assert_eq!(result.not_formatted_reason(), Some(NotFormattedReason::ApprovalRequired));
        let denied = result.approvals()[0].clone();
        assert!(denied.denied);
        assert!(client.test_client.take_shown_messages().is_empty());

        // always denying it is remembered
        let always_denied = client.approve(&denied.id, ApprovalDecision::AlwaysDeny).await;
        assert_eq!(always_denied[0].previous_decision, Some(ApprovalDecision::AlwaysDeny));

        // allowing it sets it up, and it's remembered with its configuration
        assert_eq!(client.approve(&denied.id, ApprovalDecision::AlwaysAllow).await, Vec::new());
        // the most recent decision replaces the one for its configuration
        let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert!(decisions.last().unwrap().allowed);
        assert_eq!(client.take_config_changes(), vec![uri("/a/dprint.json")]);
        assert_eq!(client.format("/a/file.txt_ps", "text").await, formatted("text", "text_formatted_process"));

        // it needs to be approved again once its configuration changes
        environment
          .write_file(
            "/a/dprint.json",
            &environment
              .read_file("/a/dprint.json")
              .unwrap()
              .replacen('{', r#"{ "testProcessPlugin": { "ending": "allowed" }, "#, 1),
          )
          .unwrap();
        let approval = client.format("/a/file.txt_ps", "text").await.approvals()[0].clone();
        assert!(approval.message.contains("changed since they were allowed."), "{}", approval.message);
        // it's set up, so what it is and what it runs with is known
        let item = &approval.items[0];
        assert_eq!(item.name.as_deref(), Some("test-process-plugin"));
        assert_eq!(item.config_key.as_deref(), Some("testProcessPlugin"));
        assert_eq!(item.configuration, Some(json!({ "ending": "allowed" })));
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(client.format("/a/file.txt_ps", "text").await, formatted("text", "text_allowed"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_forget_what_a_deleted_config_file_needed_approved() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(client.pending_approvals().await, vec![approval.clone()]);
        environment.remove_file("/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/dprint.json"),
              typ: FileChangeType::DELETED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/dprint.json")]);
        assert_eq!(client.pending_approvals().await, Vec::new());
        // it can't be approved anymore
        let err = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: approval.id.clone(),
              decision: ApprovalDecision::Allow,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ContentModified);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_return_what_needs_to_be_approved_next_when_approving() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL).add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    move |client| {
      async move {
        // both are known, so they can be asked about together
        let approvals = client.config_info("/a/dprint.json").await.status.approvals().to_vec();
        assert_eq!(
          approvals.iter().map(|approval| approval.kind).collect::<Vec<_>>(),
          vec![ApprovalKind::Download, ApprovalKind::ProcessPlugins]
        );
        assert_eq!(approvals[0].config_file, Some(uri("/a/dprint.json")));

        // allowing the download leaves the process plugin to be approved
        let remaining = client.approve(&approvals[0].id, ApprovalDecision::Allow).await;
        assert_eq!(remaining, vec![approvals[1].clone()]);

        // the download that was approved isn't pending anymore
        assert_eq!(client.format("/a/file.txt_ps", "text").await.approvals(), remaining.as_slice());

        assert_eq!(client.approve(&approvals[1].id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(client.format("/a/file.txt_ps", "text").await, formatted("text", "text_formatted_process"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_config_schema_of_a_config_file() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  environment.add_remote_file(
    "https://plugins.dprint.dev/test/schema.json",
    br#"{ "type": "object", "properties": { "ending": { "type": "string" } } }"#,
  );
  run_session(&environment, |client| {
    async move {
      let result = client
        .backend
        .dprint_config_schema(
          ConfigParams {
            uri: uri("/dprint.json"),
            global_config: None,
            work_done_token: None,
          },
          CancellationToken::new(),
        )
        .await
        .unwrap();
      assert!(result.complete);
      assert_eq!(result.schema["properties"]["test-plugin"]["$ref"], json!("#/definitions/plugin-test-plugin"));
    }
    .boxed_local()
  });
}

#[test]
fn should_not_download_a_plugin_schema_from_a_domain_that_is_not_trusted() {
  const UNTRUSTED_SCHEMA_URL: &str = "https://example.com/schema.json";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  // the plugin's schema url could be anywhere, which this redirect stands in for
  environment.add_remote_file_redirect("https://plugins.dprint.dev/test/schema.json", UNTRUSTED_SCHEMA_URL);
  environment.add_remote_file(UNTRUSTED_SCHEMA_URL, br#"{ "type": "object" }"#);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let result = client
          .backend
          .dprint_config_schema(
            ConfigParams {
              uri: uri("/dprint.json"),
              global_config: None,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap();
        // it doesn't have the plugin's schema
        assert!(!result.complete);
        assert!(result.schema["definitions"].get("plugin-test-plugin").is_none(), "{:#}", result.schema);
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_SCHEMA_URL), 0);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_start_the_plugins_again_after_a_reset() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
      client.backend.dprint_reset(Some(ResetParams {}), CancellationToken::new()).await.unwrap();
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
    }
    .boxed_local()
  });
}

#[test]
fn should_ensure_a_stable_format_with_the_setting() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      // the test plugin formats this text differently each time, which is only formatted once by default
      let unstable = "unstable_fmt_true";
      assert_eq!(client.format("/file.txt", unstable).await, formatted(unstable, "unstable_fmt_false_formatted"));
      client
        .backend
        .did_change_configuration(DidChangeConfigurationParams {
          settings: json!({ "ensureStableFormat": true }),
        })
        .await;
      assert_eq!(
        client.format("/file.txt", unstable).await,
        FormatResult::Failed {
          reason: FormatFailedReason::FormatError,
          message: "Formatting not stable. Bailed after 5 tries. This indicates a bug in the plugin where it formats the file differently each time."
            .to_string(),
          plugin: Some(test_plugin()),
        }
      );
    }
    .boxed_local()
  });
}

/// An editor extension using the `dprint/*` methods.
struct Client {
  backend: Rc<Backend<TestEnvironment>>,
  test_client: Arc<TestClient>,
}

impl Client {
  fn format_params(&self, path_or_uri: &str, text: &str) -> FormatParams {
    FormatParams {
      document: document_params(path_or_uri),
      text: text.to_string(),
      ranges: None,
      override_config: Default::default(),
    }
  }

  async fn format(&self, path_or_uri: &str, text: &str) -> FormatResult {
    self.format_with(self.format_params(path_or_uri, text)).await
  }

  async fn format_with(&self, params: FormatParams) -> FormatResult {
    self.backend.dprint_format(params, CancellationToken::new()).await.unwrap()
  }

  async fn file_info(&self, path_or_uri: &str) -> FileInfoResult {
    self
      .backend
      .dprint_file_info(
        FileInfoParams {
          document: document_params(path_or_uri),
          text: None,
        },
        CancellationToken::new(),
      )
      .await
      .unwrap()
  }

  async fn config_info(&self, path_or_uri: &str) -> ConfigInfoResult {
    self
      .backend
      .dprint_config_info(
        ConfigParams {
          uri: uri(path_or_uri),
          global_config: None,
          work_done_token: None,
        },
        CancellationToken::new(),
      )
      .await
      .unwrap()
  }

  async fn approve(&self, id: &str, decision: ApprovalDecision) -> Vec<PendingApproval> {
    self
      .backend
      .dprint_approve(
        ApproveParams {
          id: id.to_string(),
          decision,
          work_done_token: None,
        },
        CancellationToken::new(),
      )
      .await
      .unwrap()
      .approvals
  }

  async fn pending_approvals(&self) -> Vec<PendingApproval> {
    self
      .backend
      .dprint_pending_approvals(Some(PendingApprovalsParams {}), CancellationToken::new())
      .await
      .unwrap()
      .approvals
  }

  fn take_config_changes(&self) -> Vec<Uri> {
    self.test_client.config_changes.lock().drain(..).collect()
  }

  /// Waits for the config changes, which are sent a little after the editor
  /// says a config file changed.
  async fn wait_for_config_changes(&self) -> Vec<Uri> {
    for _ in 0..200 {
      let changes = self.take_config_changes();
      if !changes.is_empty() {
        return changes;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Vec::new()
  }

  /// Waits for diagnostics to be published.
  async fn wait_for_published_diagnostics(&self) -> Vec<(Uri, Vec<Diagnostic>)> {
    for _ in 0..200 {
      let diagnostics = self.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
      if !diagnostics.is_empty() {
        return diagnostics;
      }
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Vec::new()
  }
}

/// Runs a session of an editor extension, which is initialized with a
/// workspace folder at the root directory.
fn run_session(environment: &TestEnvironment, action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + 'static) {
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend(environment.clone());
    let backend = Rc::new(backend);
    let run_test_task = dprint_core::async_runtime::spawn({
      let test_client = test_client.clone();
      async move {
        initialize_backend(&backend, editor_extension_params()).await;
        action(Client {
          backend: backend.clone(),
          test_client,
        })
        .await;
        backend.shutdown().await.unwrap();
      }
    });
    try_join!(recv_task, run_test_task).unwrap();
    // what's logged is sent to the editor
    test_client.take_messages();
  });
}

fn editor_extension_params() -> InitializeParams {
  InitializeParams {
    capabilities: ClientCapabilities {
      experimental: Some(json!({ "dprint": { "versions": [1] } })),
      window: Some(WindowClientCapabilities {
        work_done_progress: Some(true),
        ..Default::default()
      }),
      ..Default::default()
    },
    workspace_folders: Some(vec![WorkspaceFolder {
      uri: uri("/"),
      name: "root".to_string(),
    }]),
    ..Default::default()
  }
}

fn document_params(path_or_uri: &str) -> DocumentParams {
  DocumentParams {
    text_document: DocumentIdentifier { uri: uri(path_or_uri) },
    language: None,
    notebook_document: None,
    global_config: None,
    work_done_token: None,
  }
}

fn formatted(text: &str, formatted_text: &str) -> FormatResult {
  let line_index = LineIndex::new(text);
  FormatResult::Formatted {
    edits: to_lsp_edits(crate::commands::lsp::text::get_utf16_edits(text, formatted_text), &line_index),
  }
}

fn not_formatted(reason: NotFormattedReason, message: Option<&str>) -> FormatResult {
  FormatResult::NotFormatted(NotFormatted {
    reason,
    message: message.map(|message| message.to_string()),
    approvals: Vec::new(),
  })
}

fn not_formatted_status(reason: NotFormattedReason, message: Option<&str>) -> FormatStatus {
  FormatStatus::NotFormatted(NotFormatted {
    reason,
    message: message.map(|message| message.to_string()),
    approvals: Vec::new(),
  })
}

fn specifiers(approval: &PendingApproval) -> Vec<&str> {
  approval.items.iter().map(|item| item.specifier.as_str()).collect()
}

/// Gets what a status says for the assertions.
trait StatusExt {
  fn reason(&self) -> Option<NotFormattedReason>;
  fn message(&self) -> Option<&str>;
  fn approvals(&self) -> &[PendingApproval];
}

impl StatusExt for FormatStatus {
  fn reason(&self) -> Option<NotFormattedReason> {
    match self {
      FormatStatus::NotFormatted(not_formatted) => Some(not_formatted.reason),
      FormatStatus::Ready => None,
    }
  }

  fn message(&self) -> Option<&str> {
    match self {
      FormatStatus::NotFormatted(not_formatted) => not_formatted.message.as_deref(),
      FormatStatus::Ready => None,
    }
  }

  fn approvals(&self) -> &[PendingApproval] {
    match self {
      FormatStatus::NotFormatted(not_formatted) => &not_formatted.approvals,
      FormatStatus::Ready => &[],
    }
  }
}

/// Gets what a format result says for the assertions.
trait FormatResultExt {
  fn not_formatted_reason(&self) -> Option<NotFormattedReason>;
  fn approvals(&self) -> &[PendingApproval];
}

impl FormatResultExt for FormatResult {
  fn not_formatted_reason(&self) -> Option<NotFormattedReason> {
    match self {
      FormatResult::NotFormatted(not_formatted) => Some(not_formatted.reason),
      _ => None,
    }
  }

  fn approvals(&self) -> &[PendingApproval] {
    match self {
      FormatResult::NotFormatted(not_formatted) => &not_formatted.approvals,
      _ => &[],
    }
  }
}

fn test_plugin() -> FilePlugin {
  FilePlugin {
    name: "test-plugin".to_string(),
    version: "0.2.0".to_string(),
    config_key: "test-plugin".to_string(),
  }
}

fn local_config(path: &str) -> ConfigFileInfo {
  let dir_path = path.rsplit_once('/').unwrap().0;
  let dir_path = dir_path.strip_suffix("/.config").unwrap_or(dir_path);
  ConfigFileInfo {
    uri: uri(path),
    dir_uri: Some(uri(if dir_path.is_empty() { "/" } else { dir_path })),
    kind: ConfigFileKind::Local,
  }
}

fn uri(path_or_uri: &str) -> Uri {
  if path_or_uri.starts_with('/') {
    Uri::from_str(&format!("file://{}", path_or_uri)).unwrap()
  } else {
    Uri::from_str(path_or_uri).unwrap()
  }
}
