//! Tests of an editor extension using the `dprint/*` methods.

use deno_tower_lsp::lsp_types::FileChangeType;
use deno_tower_lsp::lsp_types::FileEvent;
use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;
use crate::commands::lsp::workspace::messages::Action;
use crate::commands::lsp::workspace::messages::ApprovalDecision;
use crate::commands::lsp::workspace::messages::ApprovalItem;
use crate::commands::lsp::workspace::messages::ApprovalItemKind;
use crate::commands::lsp::workspace::messages::ApprovalKind;
use crate::commands::lsp::workspace::messages::ConfigFileIdentifier;
use crate::commands::lsp::workspace::messages::ConfigFileInfo;
use crate::commands::lsp::workspace::messages::ConfigFileKind;
use crate::commands::lsp::workspace::messages::DecisionsParams;
use crate::commands::lsp::workspace::messages::DocumentIdentifier;
use crate::commands::lsp::workspace::messages::FileInfoParams;
use crate::commands::lsp::workspace::messages::FilePattern;
use crate::commands::lsp::workspace::messages::FilePlugin;
use crate::commands::lsp::workspace::messages::ForgetDecisionsParams;
use crate::commands::lsp::workspace::messages::FormatFailedReason;
use crate::commands::lsp::workspace::messages::FormatStatus;
use crate::commands::lsp::workspace::messages::GlobalConfigUse;
use crate::commands::lsp::workspace::messages::NotFormatted;
use crate::commands::lsp::workspace::messages::NotebookIdentifier;
use crate::commands::lsp::workspace::messages::OtherFiles;
use crate::commands::lsp::workspace::messages::OtherFilesKind;
use crate::commands::lsp::workspace::messages::PendingApproval;
use crate::commands::lsp::workspace::messages::PendingApprovalsParams;
use crate::commands::lsp::workspace::messages::Severity;
use crate::commands::lsp::workspace::messages::ShowPolicy;
use crate::commands::not_formatted_messages::NO_PLUGIN_MESSAGE;
use crate::commands::not_formatted_messages::NOT_MATCHED_MESSAGE;

#[test]
fn should_say_it_supports_the_dprint_methods() {
  let environment = TestEnvironmentBuilder::new().build();
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend(environment.clone());
    let run_test_task = dprint_core::async_runtime::spawn(async move {
      let result = initialize_backend(&backend, editor_extension_params()).await;
      // the version it uses and where dprint's own files are
      let capability = &result.capabilities.experimental.unwrap()["dprint"];
      assert_eq!(capability["version"], json!(1));
      assert!(capability["files"]["settingsUri"].is_string(), "{}", capability);
      assert!(capability["files"]["approvalsUri"].is_string(), "{}", capability);
      assert!(capability.get("features").is_none());
      // the text is sent when formatting, so the documents aren't synced
      assert_eq!(result.capabilities.text_document_sync, None);
      assert_eq!(result.capabilities.document_formatting_provider, None);
      backend.shutdown().await.unwrap();
    });
    try_join!(recv_task, run_test_task).unwrap();
    test_client.take_messages();
    assert!(test_client.take_registered_methods().is_empty());
  });

  // an editor extension that doesn't support a version the server does is
  // told the server's versions instead of being handled like a language client
  for (experimental, is_editor_extension) in [
    (None, false),
    (Some(json!({ "dprint": { "versions": [1, 2] } })), true),
    // what can't be read is ignored
    (Some(json!({ "dprint": { "versions": [1, "2"], "features": [] } })), true),
    (Some(json!({ "dprint": { "versions": [99] } })), false),
    (Some(json!({ "dprint": { "versions": [] } })), false),
    (Some(json!({ "dprint": { "version": 1 } })), false),
    // null is the same as not providing it
    (Some(json!({ "dprint": null })), false),
  ] {
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
            assert_eq!(result.capabilities.experimental.unwrap()["dprint"]["version"], json!(1));
          } else if experimental
            .as_ref()
            .and_then(|experimental| experimental.get("dprint"))
            .is_some_and(|capability| !capability.is_null())
          {
            // the versions it supports, so the client can tell it apart from an older dprint
            let capability = &result.capabilities.experimental.unwrap()["dprint"];
            assert_eq!(capability["versions"], json!([1]), "{:?}", experimental);
            assert!(capability.get("version").is_none());
            assert!(capability["message"].as_str().unwrap().contains("Update"), "{}", capability);
            // its language client library would start using what's advertised
            assert!(result.capabilities.text_document_sync.is_none());
            assert!(result.capabilities.document_formatting_provider.is_none());
          } else {
            assert_eq!(result.capabilities.experimental, None);
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
      Backend::new(ClientWrapper::new(Arc::new(client)), environment.clone(), tx, false)
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
    assert_eq!(response["result"]["capabilities"]["experimental"]["dprint"]["version"], json!(1));
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
          "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": false },
          "result": {
            "kind": "formatted",
            "edits": [{
              "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 4 } },
              "newText": "_formatted",
            }],
          },
        },
      })
    );
    assert_eq!(
      connection
        .request(3, "dprint/fileInfo", json!({ "textDocument": { "uri": "file:///file.txt" } }))
        .await["result"],
      json!({
        "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": false },
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
      connection
        .request(4, "dprint/configInfo", json!({ "configFile": { "uri": "file:///dprint.json" } }))
        .await["result"],
      json!({
        "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": false },
        "plugins": [{
          "name": "test-plugin",
          "version": "0.2.0",
          "configKey": "test-plugin",
          "fileExtensions": ["txt"],
          "fileNames": [],
          "additive": false,
          "rangeFormatting": "wholeLines",
          "helpUrl": "https://dprint.dev/plugins/test",
          "updateUrl": "https://plugins.dprint.dev/dprint/test-plugin/latest.json",
        }],
        "filePatterns": [{ "glob": "**/*.[tT][xX][tT]", "configKeys": ["test-plugin"] }],
        "otherFiles": [],
        "watchUris": [],
        "status": { "kind": "ready" },
      })
    );
    assert_eq!(
      connection.request(5, "dprint/approve", json!({ "id": "unknown", "decision": "allow" })).await["error"]["code"],
      json!(-32803)
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
    assert_eq!(
      connection
        .request(10, "dprint/pendingApprovals", json!({ "configFile": { "uri": "file:///dprint.json" } }))
        .await["result"],
      json!({
        "approvals": [],
        "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///", "kind": "local", "inWorkspace": false },
        "status": { "kind": "ready" },
      })
    );
    assert_eq!(
      connection.request(11, "dprint/forgetDecisions", json!({})).await["result"],
      json!({ "decisions": [] })
    );
    assert_eq!(
      connection.request(12, "dprint/decisions", json!({})).await["result"],
      json!({ "decisions": [], "allowProcessPlugins": false })
    );
    assert_eq!(connection.request(9, "dprint/reset", json!({})).await["result"], json!({}));

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
          status: FormatStatus::ready(),
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
      assert_eq!(
        client.format("/a/file.txt", "testing_a").await,
        FormatResult::Formatted {
          edits: Vec::new(),
          notice: None,
        }
      );
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
          show: ShowPolicy::Command,
          severity: Severity::Warning,
          claim: true,
          actions: Vec::new(),
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
      // the associations are in the patterns
      assert_eq!(
        config_info.file_patterns,
        vec![
          file_pattern("**/*.[tT][xX][tT]", &["test-plugin"]),
          file_pattern("**/*.other", &["test-plugin"])
        ]
      );
      assert!(config_info.other_files.is_empty());
      assert_eq!(config_info.status, FormatStatus::ready());
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
      assert!(message.contains("enable the \"useGlobalConfig\" setting of the dprint extension"));
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
      assert_eq!(info.status, FormatStatus::ready());
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
      assert_eq!(config_file.dir_uri, uri("/a"));
    }
    .boxed_local()
  });
}

#[test]
fn should_provide_the_directory_of_a_config_file_in_the_home_directorys_config_directory() {
  // the home directory's `.config` is an ordinary directory
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/home/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let config_file = client.config_info("/home/.config/file.txt").await.config_file.unwrap();
      assert_eq!(config_file.uri, uri("/home/.config/dprint.json"));
      assert_eq!(config_file.dir_uri, uri("/home/.config"));
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

      // a value the plugin would otherwise say is a problem with the config file
      for (key, value) in [
        ("indentWidth", ConfigKeyValue::from_str("4")),
        ("indentWidth", ConfigKeyValue::from_i32(0)),
        ("useTabs", ConfigKeyValue::from_i32(1)),
      ] {
        let mut params = client.format_params("/file.txt", "text");
        params.override_config = ConfigKeyMap::from([(key.to_string(), value)]);
        let err = client.backend.dprint_format(params, CancellationToken::new()).await.unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams, "{}", key);
      }
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
          notice: None,
        }
      );
      let mut params = client.format_params("/file.txt", "a\nb\nc\n");
      params.ranges = Some(vec![
        Range::new(Position::new(0, 0), Position::new(0, 1)),
        Range::new(Position::new(2, 0), Position::new(2, 1)),
      ]);
      let FormatResult::Formatted { edits, .. } = client.format_with(params).await else {
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
      // an extension may be provided without the leading period
      let mut params = client.format_params("untitled:Untitled-1", "text");
      params.document.language = Some(DocumentLanguage {
        id: "unknown".to_string(),
        file_extensions: vec!["txt".to_string()],
        file_names: Vec::new(),
      });
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
      params.document.notebook_document = Some(NotebookIdentifier {
        uri: uri("/notebook.ipynb"),
        file_uri: None,
      });
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
fn should_tell_the_editor_when_the_config_file_the_server_was_started_with_changed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/config/my-config.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  run_session_with_config(&environment, editor_extension_params(), Some(PathBuf::from("/config/my-config.json")), {
    let environment = environment.clone();
    move |client| {
      async move {
        let mut params = config_params("/dprint.json");
        params.config_file = Some(ConfigFileIdentifier {
          uri: uri("/config/my-config.json"),
          dir_uri: None,
        });
        let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
        assert_eq!(config_info.status, FormatStatus::ready());
        let change = |path: &str| DidChangeWatchedFilesParams {
          changes: vec![FileEvent {
            uri: uri(path),
            typ: FileChangeType::CHANGED,
          }],
        };
        // another config file isn't one to the server
        environment.mk_dir_all("/project").unwrap();
        environment.write_file("/project/dprint.json", "{}").unwrap();
        client.backend.did_change_watched_files(change("/project/dprint.json")).await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(client.take_config_changes(), Vec::new());
        // a change to it is a change to every config file because it's used for every file
        client.backend.did_change_watched_files(change("/config/my-config.json")).await;
        assert!(client.wait_for_config_changes().await.contains(&uri("/")));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_tell_the_editor_when_a_config_file_starts_failing_because_of_its_global_config() {
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
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
        let text = environment.read_file("/dprint.json").unwrap();
        environment
          .write_file("/dprint.json", &text.replacen('{', r#"{ "lineWidth": "abc","#, 1))
          .unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/dprint.json")]);
        assert_eq!(client.config_info("/dprint.json").await.status.reason(), Some(NotFormattedReason::ConfigError));
      }
      .boxed_local()
    }
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
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());

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
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
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
      let config_info = client.config_info("/a/dprint.json").await;
      assert_eq!(
        config_info.file_patterns.iter().map(|pattern| pattern.glob.as_str()).collect::<Vec<_>>(),
        vec!["**/*.[tT][xX][tT]", "**/*.foo", "src/*.bar", "b/*.baz"]
      );
      // what can't be made relative to the directory
      assert_eq!(
        config_info.other_files,
        vec![OtherFiles {
          kind: OtherFilesKind::Associations,
          values: vec!["../c/*.qux".to_string()],
        }]
      );
    }
    .boxed_local()
  });
}

#[test]
fn should_resolve_the_config_files_again_when_dprints_own_files_change() {
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
        assert_eq!(
          client.config_info("/dprint.json").await.status.reason(),
          Some(NotFormattedReason::ApprovalRequired)
        );
        // the user trusts the domain in dprint's settings file, which the editor doesn't watch
        let settings_file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
        environment.mk_dir_all(settings_file_path.parent().unwrap()).unwrap();
        environment
          .write_file(&settings_file_path, r#"{ "editor": { "trustedDomains": ["example.com"] } }"#)
          .unwrap();
        // the config file resolves to something else now
        assert_eq!(client.wait_for_dprint_files_change().await, vec![uri("/dprint.json")]);
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
        assert_eq!(client.pending_approvals().await, Vec::new());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_resolve_a_deleted_config_file_again_when_dprints_own_files_change() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_global_config(|c| {
      c.add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
        environment.remove_file("/a/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/a/dprint.json"),
              typ: FileChangeType::DELETED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        // the settings change, which doesn't have the global config file be used for the directory
        let settings_file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
        environment
          .write_file(&settings_file_path, r#"{ "editor": { "allowProcessPlugins": false, "trustedDomains": [] } }"#)
          .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert_eq!(client.pending_approvals().await, Vec::new());
        assert_eq!(client.take_config_changes(), Vec::new());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_get_the_global_config_file_when_asked_about_it() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_process_plugin()
    .with_global_config(|c| {
      c.add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  let global_config_path = crate::configuration::resolve_global_config_path_and_text(&environment)
    .unwrap()
    .unwrap()
    .source
    .display();
  run_session(&environment, move |client| {
    async move {
      let mut params = config_params("/dprint.json");
      params.config_file = Some(ConfigFileIdentifier {
        uri: file_path_to_uri(Path::new(&global_config_path)).unwrap(),
        dir_uri: None,
      });
      let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
      assert_eq!(config_info.config_file.unwrap().kind, ConfigFileKind::Global);
      // its process plugins don't need to be approved because it's the user's own
      assert_eq!(config_info.status, FormatStatus::ready());
      assert_eq!(client.pending_approvals().await, Vec::new());
      // a file that isn't a config file
      let mut params = config_params("/dprint.json");
      params.config_file = Some(ConfigFileIdentifier {
        uri: uri("/package.json"),
        dir_uri: None,
      });
      let err = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
      // or a name that isn't found in a .config directory
      let err = client
        .backend
        .dprint_config_info(config_params("/project/.config/.dprint.json"), CancellationToken::new())
        .await
        .unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
    }
    .boxed_local()
  });
}

#[test]
fn should_ask_again_once_the_always_decisions_are_deleted() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(client.approve(&approval.id, ApprovalDecision::AlwaysAllow).await, Vec::new());
        assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
        // the server notices that they were stored
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        client.take_config_changes();
        // the user deletes them to be asked again
        environment
          .remove_file(crate::configuration::get_editor_approvals_file_path(&environment).unwrap())
          .unwrap();
        assert_eq!(client.wait_for_dprint_files_change().await, vec![uri("/a/dprint.json")]);
        assert_eq!(
          client.config_info("/a/dprint.json").await.status.reason(),
          Some(NotFormattedReason::ApprovalRequired)
        );
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_get_the_config_file_of_a_config_file_or_a_document() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  environment.write_file("/dprint.jsonc", r#"{ "plugins": [] }"#).unwrap();
  run_session(&environment, |client| {
    async move {
      // the config file that's asked about
      let config_info = client.config_info("/dprint.jsonc").await;
      assert_eq!(config_info.config_file.unwrap().uri, uri("/dprint.jsonc"));
      // the config file of an untitled document
      let mut params = config_params("untitled:Untitled-1");
      params.language = Some(DocumentLanguage {
        id: "plaintext".to_string(),
        file_extensions: vec![".txt".to_string()],
        file_names: Vec::new(),
      });
      let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
      assert_eq!(config_info.config_file.unwrap().uri, uri("/dprint.json"));
      // only one of them
      let mut params = config_params("/dprint.json");
      params.text_document = Some(DocumentIdentifier { uri: uri("/file.txt") });
      let err = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
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
      assert_ne!(client.file_info("/script").await.status, FormatStatus::ready());
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
      assert_eq!(file_info.status, FormatStatus::ready());
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

        // an error offers to open the config file
        environment.write_file("/dprint.json", "{").unwrap();
        let FormatStatus::NotFormatted(not_formatted) = client.config_info("/dprint.json").await.status else {
          panic!("expected it to not be formatted");
        };
        assert_eq!(not_formatted.reason, NotFormattedReason::ConfigError);
        assert_eq!(
          not_formatted.actions,
          vec![Action::OpenUri {
            label: "Open Configuration File".to_string(),
            uri: uri("/dprint.json"),
          }]
        );
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
      assert_eq!(config_info.status, FormatStatus::ready());
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
      assert!(config_info.file_patterns.iter().any(|pattern| pattern.glob == "**/*.other"));
    }
    .boxed_local()
  });
}

#[test]
fn should_only_claim_the_documents_a_config_file_that_needs_approval_formats() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "associations": ["**/*.other"] }"#)
        .add_excludes("excluded.txt");
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      // what the plugins format isn't known until they're set up
      let status = client.file_info("/file.txt").await.status;
      assert_eq!(status.reason(), Some(NotFormattedReason::ApprovalRequired));
      assert!(!status.claim());
      let approval = status.approvals()[0].clone();
      assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      // what they format is remembered once the editor gets it again
      assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
  // a new session needs them approved again, which is only asked about for what they format
  run_session(&environment, |client| {
    async move {
      for path in ["/file.txt", "/file.txt_ps", "/file.other"] {
        let status = client.file_info(path).await.status;
        assert_eq!(status.reason(), Some(NotFormattedReason::ApprovalRequired), "{}", path);
        assert!(status.claim(), "{}", path);
      }
      let status = client.file_info("/readme.md").await.status;
      assert_eq!(status.reason(), Some(NotFormattedReason::NoPlugin));
      assert!(!status.claim());
      assert_eq!(client.file_info("/excluded.txt").await.status.reason(), Some(NotFormattedReason::NotMatched));
      assert!(client.format("/readme.md", "text").await.approvals().is_empty());

      // what was denied isn't claimed, which the editor is told about
      let approval = client.file_info("/file.txt").await.status.approvals()[0].clone();
      client.approve(&approval.id, ApprovalDecision::Deny).await;
      assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
      assert!(!client.file_info("/file.txt").await.status.claim());
    }
    .boxed_local()
  });
}

#[test]
fn should_not_provide_the_dprint_methods_to_a_language_client() {
  let environment = TestEnvironmentBuilder::new().build();
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend(environment.clone());
    let run_test_task = dprint_core::async_runtime::spawn(async move {
      initialize_backend(&backend, InitializeParams::default()).await;
      let err = backend
        .dprint_file_info(
          FileInfoParams {
            document: document_params("/file.txt"),
            text: None,
          },
          CancellationToken::new(),
        )
        .await
        .unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::MethodNotFound);
      backend.shutdown().await.unwrap();
    });
    try_join!(recv_task, run_test_task).unwrap();
    test_client.take_messages();
  });
}

#[test]
fn should_format_a_document_as_another_file_with_the_file_uri_feature() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .initialize()
    .build();
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "fileUri": true, "unknown": true } } }));
  run_session_with(&environment, params, |client| {
    async move {
      let mut params = client.format_params("git:/file.txt", "text");
      params.document.file_uri = Some(uri("/file.txt"));
      assert_eq!(client.format_with(params).await, formatted("text", "text_formatted"));
    }
    .boxed_local()
  });
  // it's ignored without the feature
  run_session(&environment, |client| {
    async move {
      let mut params = client.format_params("git:/file.txt", "text");
      params.document.file_uri = Some(uri("/file.txt"));
      assert_eq!(
        client.format_with(params).await.not_formatted_reason(),
        Some(NotFormattedReason::UnsupportedDocument)
      );
    }
    .boxed_local()
  });
}

#[test]
fn should_say_which_features_it_uses() {
  let environment = TestEnvironmentBuilder::new().build();
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend(environment.clone());
    let run_test_task = dprint_core::async_runtime::spawn(async move {
      let mut params = editor_extension_params();
      params.capabilities.experimental = Some(json!({
        "dprint": { "versions": [1], "features": { "fileUri": true, "watchFiles": true, "unknown": true, "other": false } }
      }));
      let result = initialize_backend(&backend, params).await;
      // the client can't register the files to watch
      assert_eq!(result.capabilities.experimental.unwrap()["dprint"]["features"], json!({ "fileUri": true }));
      backend.shutdown().await.unwrap();
    });
    try_join!(recv_task, run_test_task).unwrap();
    test_client.take_messages();
  });
}

#[test]
fn should_register_the_files_to_watch_with_the_watch_files_feature() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_config_section("extends", r#"["../shared/base.json", "./configs/local.json"]"#);
    })
    .initialize()
    .build();
  environment.mk_dir_all("/shared").unwrap();
  environment
    .write_file("/shared/base.json", r#"{ "test-plugin": { "ending": "base" } }"#)
    .unwrap();
  environment.mk_dir_all("/a/configs").unwrap();
  environment
    .write_file("/a/configs/local.json", r#"{ "test-plugin": { "ending": "local" } }"#)
    .unwrap();
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "watchFiles": true } } }));
  params.capabilities.workspace = Some(WorkspaceClientCapabilities {
    did_change_watched_files: Some(deno_tower_lsp::lsp_types::DidChangeWatchedFilesClientCapabilities {
      dynamic_registration: Some(true),
      relative_pattern_support: Some(true),
    }),
    ..Default::default()
  });
  params.workspace_folders = Some(vec![WorkspaceFolder {
    uri: uri("/a"),
    name: "a".to_string(),
  }]);
  run_session_with(&environment, params, |client| {
    async move {
      // the config files in the workspace
      let registration_ids = || client.test_client.registrations.lock().iter().map(|r| r.id.clone()).collect::<Vec<_>>();
      assert_eq!(registration_ids(), vec!["dprint-config-file-watcher".to_string()]);
      // and the config files that were resolved with the files they were resolved from
      client.config_info("/a/dprint.json").await;
      assert_eq!(
        registration_ids(),
        vec!["dprint-config-file-watcher".to_string(), "dprint-watched-files".to_string()]
      );
      let options = client.test_client.registrations.lock()[1].register_options.clone().unwrap();
      let mut watched = options["watchers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|watcher| {
          let pattern = &watcher["globPattern"];
          format!("{}/{}", pattern["baseUri"].as_str().unwrap(), pattern["pattern"].as_str().unwrap())
        })
        .collect::<Vec<_>>();
      watched.sort();
      assert_eq!(
        watched,
        vec![
          format!("{}/local.json", uri("/a/configs").as_str()),
          format!("{}/dprint.json", uri("/a").as_str()),
          format!("{}/base.json", uri("/shared").as_str()),
        ]
      );
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
        assert_eq!(approval.kind, ApprovalKind::Downloads);
        assert_eq!(specifiers(&approval), vec![UNTRUSTED_PLUGIN_URL]);
        assert_eq!(approval.items[0].kind, ApprovalItemKind::Plugin);
        // a download is only allowed for the session
        assert_eq!(
          approval.decisions.iter().map(|option| (option.id, option.label.as_str())).collect::<Vec<_>>(),
          vec![(ApprovalDecision::Allow, "Allow"), (ApprovalDecision::Deny, "Deny")]
        );
        assert!(!approval.denied);
        assert!(
          approval
            .message
            .starts_with("dprint needs to download https://example.com/test-plugin.wasm from domains that aren't trusted.")
        );
        assert_eq!(
          client.format("/file.txt", "text").await,
          FormatResult::NotFormatted(
            NotFormatted::new(NotFormattedReason::ApprovalRequired, Some(approval.message.clone()), vec![approval.clone()]).unclaimed()
          )
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

        // denying it returns the approval that's still needed, and the editor stops claiming its documents
        let denied = client.approve(&approval.id, ApprovalDecision::Deny).await;
        assert!(denied[0].denied);
        assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
        assert!(client.config_info("/dprint.json").await.status.approvals()[0].denied);
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 0);

        // it's pending until it's allowed
        assert_eq!(client.pending_approvals().await.len(), 1);

        // the user can still allow it (ex. with a command), which says the config file is used now
        let result = client
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
          .unwrap();
        assert_eq!(result.config_approvals.approvals, Vec::new());
        assert_eq!(result.config_approvals.status, FormatStatus::ready());
        assert_eq!(result.notice, None);
        assert_eq!(client.pending_approvals().await, Vec::new());
        assert_eq!(environment.remote_file_request_count(UNTRUSTED_PLUGIN_URL), 1);
        // the editor is told the config file formats files now
        assert_eq!(client.take_config_changes(), vec![uri("/dprint.json")]);
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
        assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());

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
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ServerError(-32803));
        let data = err.data.unwrap();
        assert_eq!(data["kind"], json!("approvalChanged"));
        assert_eq!(data["current"]["approvals"], json!([]));
        assert_eq!(data["current"]["status"], json!({ "kind": "ready" }));
        assert_eq!(data["current"]["configFile"]["uri"], json!(uri("/dprint.json").as_str()));

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
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ServerError(-32803));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_approve_downloading_through_the_proxy_of_a_project_npmrc() {
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use crate::test_helpers::create_test_npm_tarball;

  const PACKUMENT_URL: &str = "https://registry.npmjs.org/test-plugin";
  const TARBALL_URL: &str = "https://registry.npmjs.org/test-plugin/-/test-plugin-1.0.0.tgz";
  const PROXY_SETTING: &str = "https://${GITHUB_TOKEN}@proxy.example";
  let environment = TestEnvironmentBuilder::new()
    .with_local_config("/project/dprint.json", |c| {
      c.add_plugin("npm:test-plugin@1.0.0");
    })
    .write_file("/project/.npmrc", format!("https-proxy={}", PROXY_SETTING))
    .build();
  environment.set_env_var("GITHUB_TOKEN", Some("secret"));
  let packument = json!({ "versions": { "1.0.0": { "dist": { "tarball": TARBALL_URL } } } });
  environment.add_remote_file_bytes(PACKUMENT_URL, packument.to_string().into_bytes());
  environment.add_remote_file_bytes(TARBALL_URL, create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]));
  let npmrc_path = Path::new("/project").join(".npmrc").display().to_string();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // the registry is trusted, but the project chose the proxy and what's sent to it
        let config_info = client.config_info("/project/dprint.json").await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
        let approvals = config_info.status.approvals();
        assert_eq!(approvals.len(), 1);
        let approval = approvals[0].clone();
        assert_eq!(approval.kind, ApprovalKind::Downloads);
        assert_eq!(approval.title, "Download through a proxy that isn't trusted");
        assert_eq!(approval.items.len(), 1);
        // the setting is shown before its environment variables are replaced
        assert_eq!(approval.items[0].specifier, PROXY_SETTING);
        assert_eq!(approval.items[0].kind, ApprovalItemKind::NpmrcProxy);
        assert_eq!(approval.items[0].npmrc_uri, Some(uri("/project/.npmrc")));
        assert!(
          approval
            .message
            .starts_with(&format!("The proxy {} that {} sets isn't trusted", PROXY_SETTING, npmrc_path)),
          "{}",
          approval.message
        );
        assert_eq!(environment.remote_file_request_count(PACKUMENT_URL), 0);

        // allowing it downloads through it
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(environment.remote_file_request_count(PACKUMENT_URL), 1);
        assert_eq!(
          environment.take_remote_file_proxy(PACKUMENT_URL).unwrap(),
          format!(
            "{:?}",
            crate::environment::DownloadProxy::Npmrc {
              url: "https://secret@proxy.example",
              source: &crate::utils::NpmrcProxy {
                npmrc_path: npmrc_path.clone(),
                setting: PROXY_SETTING.to_string(),
              },
            }
          )
        );
        assert_eq!(client.take_config_changes(), vec![uri("/project/dprint.json")]);
        assert_eq!(client.format("/project/file.txt", "text").await, formatted("text", "text_formatted"));

        // the user can see what they allowed
        let decisions = client
          .backend
          .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        assert_eq!(decisions.decisions.len(), 1);
        assert_eq!(decisions.decisions[0].kind, ApprovalKind::Downloads);
        // its specifier is the setting like the item's, and the item says it's a proxy and of which file
        assert_eq!(decisions.decisions[0].specifiers, vec![PROXY_SETTING.to_string()]);
        assert_eq!(decisions.decisions[0].items, Some(approval.items.clone()));
        assert!(decisions.decisions[0].allows);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_approve_the_proxy_of_a_project_npmrc_on_its_own() {
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use crate::test_helpers::create_test_npm_tarball;

  const PACKUMENT_URL: &str = "https://npm.example.com/test-plugin";
  const TARBALL_URL: &str = "https://npm.example.com/test-plugin/-/test-plugin-1.0.0.tgz";
  const PROXY_SETTING: &str = "https://${GITHUB_TOKEN}@proxy.example";
  // the registry isn't trusted, and the project chose the proxy too
  let environment = TestEnvironmentBuilder::new()
    .with_local_config("/project/dprint.json", |c| {
      c.add_plugin("npm:test-plugin@1.0.0");
    })
    .write_file("/project/.npmrc", format!("registry=https://npm.example.com/\nhttps-proxy={}", PROXY_SETTING))
    .build();
  environment.set_env_var("GITHUB_TOKEN", Some("secret"));
  let packument = json!({ "versions": { "1.0.0": { "dist": { "tarball": TARBALL_URL } } } });
  environment.add_remote_file_bytes(PACKUMENT_URL, packument.to_string().into_bytes());
  environment.add_remote_file_bytes(TARBALL_URL, create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]));
  let npmrc_path = Path::new("/project").join(".npmrc").display().to_string();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let config_info = client.config_info("/project/dprint.json").await;
        let approvals = config_info.status.approvals();
        assert_eq!(approvals.len(), 2);
        // the proxy is its own approval, which is first and only about it
        let proxy_approval = approvals[0].clone();
        assert_eq!(proxy_approval.kind, ApprovalKind::Downloads);
        assert_eq!(proxy_approval.title, "Download through a proxy that isn't trusted");
        assert_eq!(
          proxy_approval.items,
          vec![ApprovalItem {
            specifier: PROXY_SETTING.to_string(),
            kind: ApprovalItemKind::NpmrcProxy,
            registry: None,
            npmrc_uri: Some(uri("/project/.npmrc")),
            plugin: None,
            configuration: None,
          }]
        );
        assert!(
          proxy_approval
            .message
            .starts_with(&format!("The proxy {} that {} sets isn't trusted", PROXY_SETTING, npmrc_path)),
          "{}",
          proxy_approval.message
        );
        assert!(!proxy_approval.message.contains("npm.example.com"), "{}", proxy_approval.message);
        let url_approval = approvals[1].clone();
        assert_eq!(url_approval.kind, ApprovalKind::Downloads);
        assert_eq!(url_approval.title, "Download plugins from domains that aren't trusted");
        assert_eq!(url_approval.items.len(), 1);
        assert_eq!(url_approval.items[0].kind, ApprovalItemKind::Plugin);
        let registry_url = url_approval.items[0].specifier.clone();
        assert!(registry_url.starts_with("https://npm.example.com"), "{}", registry_url);
        assert!(!url_approval.message.contains("proxy"), "{}", url_approval.message);
        assert_ne!(proxy_approval.id, url_approval.id);
        // the message of the result says the proxy first too
        let FormatResult::NotFormatted(not_formatted) = client.format("/project/file.txt", "text").await else {
          panic!("it shouldn't be formatted");
        };
        assert!(not_formatted.message.starts_with("The proxy "), "{}", not_formatted.message);

        // allowing the url reveals that the proxy is still needed, with the same id
        assert_eq!(client.approve(&url_approval.id, ApprovalDecision::Allow).await, vec![proxy_approval.clone()]);
        assert_eq!(environment.remote_file_request_count(PACKUMENT_URL), 0);

        // denying the proxy only denies it
        let denied = client.approve(&proxy_approval.id, ApprovalDecision::Deny).await;
        assert_eq!(denied.len(), 1);
        assert_eq!(denied[0].id, proxy_approval.id);
        assert!(denied[0].denied);
        assert_eq!(environment.remote_file_request_count(PACKUMENT_URL), 0);
        let decisions = client
          .backend
          .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap()
          .decisions;
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].specifiers, vec![PROXY_SETTING.to_string()]);
        assert_eq!(decisions[0].items, Some(proxy_approval.items.clone()));
        assert!(!decisions[0].allows);
        assert_eq!(decisions[1].specifiers, vec![registry_url.clone()]);
        assert_eq!(decisions[1].items, Some(url_approval.items.clone()));
        assert!(decisions[1].allows);

        // the user can still allow it (ex. with a command), which downloads through it
        assert_eq!(client.approve(&proxy_approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(environment.remote_file_request_count(PACKUMENT_URL), 1);
        assert_eq!(client.format("/project/file.txt", "text").await, formatted("text", "text_formatted"));
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
      assert_eq!(approval.items[0].kind, ApprovalItemKind::Config);
      // allowing it only allows what was shown, so what it extends needs to be approved too
      let approval = client.approve(&approval.id, ApprovalDecision::Allow).await[0].clone();
      assert_eq!(approval.kind, ApprovalKind::Downloads);
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
        assert_eq!(approval.config_file, local_config("/a/dprint.json"));
        assert_eq!(approval.decisions.len(), 4);
        assert_eq!(approval.items.len(), 1);
        assert!(approval.items[0].specifier.starts_with("https://plugins.dprint.dev/test-process.json@"));
        // what it is and its configuration isn't known until it's set up
        assert_eq!(approval.items[0].plugin, None);
        assert_eq!(approval.items[0].configuration, None);
        assert!(approval.message.contains(") weren't allowed to run."), "{}", approval.message);
        // none of the plugins are set up until then
        assert_eq!(client.config_info("/a/dprint.json").await.plugins, None);

        // denying it is remembered for the session, which a format says
        let denied = client.approve(&approval.id, ApprovalDecision::Deny).await;
        assert!(denied[0].denied);
        assert_eq!(denied[0].previous_decision, Some(ApprovalDecision::Deny));
        assert_eq!(client.take_config_changes(), vec![uri("/a/dprint.json")]);
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
        let plugin = item.plugin.as_ref().unwrap();
        assert_eq!(plugin.name, "test-process-plugin");
        assert_eq!(plugin.config_key, "testProcessPlugin");
        assert_eq!(item.configuration, Some(json!({ "ending": "allowed" })));
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(client.format("/a/file.txt_ps", "text").await, formatted("text", "text_allowed"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_change_the_id_of_a_process_plugin_that_is_not_set_up_when_the_configuration_does() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "ending": "first" }"#);
    })
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        // it isn't set up, so what it runs with isn't known yet
        assert_eq!(approval.items[0].configuration, None);
        // so what it runs with is the trusted configuration of the config file
        assert_eq!(
          approval.trusted_configuration.as_ref().unwrap()["testProcessPlugin"],
          json!({ "ending": "first" })
        );
        assert_eq!(approval.title, "Run process plugins");
        let text = environment.read_file("/dprint.json").unwrap();
        environment.write_file("/dprint.json", &text.replace("first", "second")).unwrap();
        let changed = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_ne!(changed.id, approval.id);
        // the user didn't see what it would run with now
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
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ServerError(-32803));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_show_the_registry_of_an_npm_process_plugin_that_is_not_the_default_one() {
  const PLUGIN: &str = "npm:@dprint/exec@0.5.0/plugin.json@abc123";
  // gets the approval of the process plugin when the project's .npmrc has the text
  fn get_approval(npmrc: Option<&str>) -> PendingApproval {
    let mut builder = TestEnvironmentBuilder::new();
    builder.with_local_config("/project/dprint.json", |c| {
      c.add_plugin(PLUGIN);
    });
    if let Some(npmrc) = npmrc {
      builder.write_file("/project/.npmrc", npmrc);
    }
    let environment = builder.build();
    disallow_process_plugins(&environment);
    let approval = Rc::new(std::cell::RefCell::new(None));
    run_session(&environment, {
      let approval = approval.clone();
      move |client| {
        async move {
          let approvals = client.config_info("/project/dprint.json").await.status.approvals().to_vec();
          assert_eq!(approvals.len(), 1, "{:#?}", approvals);
          *approval.borrow_mut() = Some(approvals[0].clone());
        }
        .boxed_local()
      }
    });
    approval.take().unwrap()
  }

  // a trusted domain doesn't need a download approval, so this is what the user sees of where it's from
  let approval = get_approval(Some("@dprint:registry=https://raw.githubusercontent.com/other/registry/main/"));
  assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
  assert_eq!(approval.items[0].specifier, PLUGIN);
  assert_eq!(
    approval.items[0].registry.as_deref(),
    Some("https://raw.githubusercontent.com/other/registry/main")
  );
  assert!(
    approval
      .message
      .contains(&format!("({} from https://raw.githubusercontent.com/other/registry/main)", PLUGIN)),
    "{}",
    approval.message
  );
  // what was shown is part of the id
  let other_registry = get_approval(Some("@dprint:registry=https://raw.githubusercontent.com/another/registry/main/"));
  assert_ne!(other_registry.id, approval.id);
  // the default registry isn't shown
  let default_registry = get_approval(None);
  assert_eq!(default_registry.items[0].registry, None);
  assert!(default_registry.message.contains(&format!("({})", PLUGIN)), "{}", default_registry.message);
  assert_ne!(default_registry.id, approval.id);
  assert_eq!(get_approval(Some("registry=https://registry.npmjs.org/")).id, default_registry.id);
}

#[test]
fn should_tell_the_editor_when_the_settings_change_what_is_formatted() {
  let environment = TestEnvironmentBuilder::new().build();
  run_session(&environment, |client| {
    async move {
      assert_eq!(client.take_config_changes(), Vec::new());
      client
        .backend
        .did_change_configuration(DidChangeConfigurationParams {
          settings: json!({ "verbose": true }),
        })
        .await;
      assert_eq!(client.take_config_changes(), Vec::new());
      // the documents without a config file may use the global config file now
      client
        .backend
        .did_change_configuration(DidChangeConfigurationParams {
          settings: json!({ "useGlobalConfig": true }),
        })
        .await;
      // every config file may have changed
      assert_eq!(client.take_config_changes(), vec![uri("/")]);
    }
    .boxed_local()
  });
}

#[test]
fn should_use_the_other_config_files_that_needed_a_download_once_it_is_allowed() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/a/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(
        client.config_info("/b/dprint.json").await.status.reason(),
        Some(NotFormattedReason::ApprovalRequired)
      );
      assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      // the other config file is resolved again after responding
      let mut changes = client.take_config_changes();
      while !changes.contains(&uri("/b/dprint.json")) {
        changes.extend(client.wait_for_config_changes().await);
      }
      changes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
      changes.dedup();
      assert_eq!(changes, vec![uri("/a/dprint.json"), uri("/b/dprint.json")]);
      assert_eq!(client.config_info("/b/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_forget_the_decisions_of_a_config_file() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        let denied = client.approve(&approval.id, ApprovalDecision::AlwaysDeny).await;
        assert!(denied[0].denied);
        client.take_config_changes();
        let stored = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert_eq!(stored.len(), 1);
        // the user can see what they decided
        let decisions = client
          .backend
          .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        assert!(!decisions.allow_process_plugins);
        assert_eq!(decisions.decisions.len(), 1);
        assert_eq!(decisions.decisions[0].kind, ApprovalKind::ProcessPlugins);
        assert_eq!(
          decisions.decisions[0].config_file,
          Some(ConfigFileIdentifier {
            uri: uri("/a/dprint.json"),
            dir_uri: Some(uri("/a")),
          })
        );
        assert!(!decisions.decisions[0].allows);
        assert!(decisions.decisions[0].always);

        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/a/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(forgotten.decisions, decisions.decisions);
        // the user is asked again
        assert_eq!(client.take_config_changes(), vec![uri("/a/dprint.json")]);
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert!(!approval.denied);
        assert!(
          crate::configuration::read_editor_approvals(&environment)
            .unwrap()
            .process_plugin_decisions
            .is_empty()
        );

        // a deleted config file's decisions can be forgotten too
        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/b/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_most_recent_decision_of_a_process_plugin() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        environment.set_fs_time(1);
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        let denied = client.approve(&approval.id, ApprovalDecision::Deny).await;
        assert!(denied[0].denied);
        // it's allowed after (ex. by running `dprint fmt`), which is stored
        environment.set_fs_time(2);
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: "/a/dprint.json".to_string(),
            plugin: approval.items[0].specifier.clone(),
            config_hash: Some("hash".to_string()),
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        let decisions = client
          .backend
          .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        assert_eq!(decisions.decisions.len(), 1);
        assert!(decisions.decisions[0].allows);
        assert!(decisions.decisions[0].always);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_forget_the_decisions_of_a_remote_config_file() {
  let environment = TestEnvironmentBuilder::new().initialize().build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // ex. from running `dprint fmt --config https://example.com/dprint.json`
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: "https://example.com/dprint.json".to_string(),
            plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
            config_hash: Some("1:hash".to_string()),
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        let decisions = client
          .backend
          .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap()
          .decisions;
        let config_file = decisions[0].config_file.clone().unwrap();
        assert_eq!(config_file.uri.as_str(), "https://example.com/dprint.json");
        let params = ForgetDecisionsParams {
          config_file: Some(config_file),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(forgotten.decisions, decisions);
        assert!(
          crate::configuration::read_editor_approvals(&environment)
            .unwrap()
            .process_plugin_decisions
            .is_empty()
        );
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_id_of_a_process_plugin_approval_before_it_was_set_up() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build(); // not initialized, so the process plugin is only set up when it's allowed
  disallow_process_plugins(&environment);
  let id_before = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
  run_session(&environment, {
    let id_before = id_before.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(approval.id_before_set_up, None);
        *id_before.borrow_mut() = approval.id.clone();
        // allowing it for the session sets it up
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      }
      .boxed_local()
    }
  });
  // a server that's started again asks about the plugin with its configuration, which was
  // allowed with the same trusted configuration before it was set up
  run_session(&environment, move |client| {
    async move {
      let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
      assert_ne!(approval.id, *id_before.borrow());
      assert_eq!(approval.id_before_set_up.as_deref(), Some(id_before.borrow().as_str()));
    }
    .boxed_local()
  });
}

#[test]
fn should_change_the_id_of_an_unpinned_npm_process_plugin_approval_when_its_file_changes() {
  let environment = TestEnvironmentBuilder::new()
    .with_local_config("/dprint.json", |c| {
      c.add_plugin("npm:test-process/plugin.json");
    })
    .write_file("/node_modules/test-process/plugin.json", "{}")
    .build();
  disallow_process_plugins(&environment);
  let get_approval = |environment: &TestEnvironment| {
    let approval = std::rc::Rc::new(std::cell::RefCell::new(None));
    run_session(environment, {
      let approval = approval.clone();
      move |client| {
        async move {
          *approval.borrow_mut() = Some(client.config_info("/dprint.json").await.status.approvals()[0].clone());
        }
        .boxed_local()
      }
    });
    approval.take().unwrap()
  };
  let approval = get_approval(&environment);
  assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
  assert_eq!(approval.id_before_set_up, None);
  assert_eq!(get_approval(&environment).id, approval.id);
  // ex. `npm install` updated it after the user allowed it for the session, so it isn't what they allowed
  environment
    .write_file("/node_modules/test-process/plugin.json", r#"{ "name": "other" }"#)
    .unwrap();
  assert_ne!(get_approval(&environment).id, approval.id);
}

#[test]
fn should_set_up_an_allowed_process_plugin_once_what_needs_to_be_downloaded_is_allowed() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL).add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
      assert_eq!(
        approvals.iter().map(|approval| approval.kind).collect::<Vec<_>>(),
        vec![ApprovalKind::Downloads, ApprovalKind::ProcessPlugins]
      );
      // the process plugins are allowed first, which can't be set up until the download is allowed
      let remaining = client.approve(&approvals[1].id, ApprovalDecision::Allow).await;
      assert_eq!(
        remaining.iter().map(|approval| approval.kind).collect::<Vec<_>>(),
        vec![ApprovalKind::Downloads]
      );
      assert_eq!(client.approve(&approvals[0].id, ApprovalDecision::Allow).await, Vec::new());
      assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_not_use_a_process_plugin_that_was_denied_while_it_was_set_up() {
  const ZIP_URL: &str = "https://github.com/dprint/test-process-plugin/releases/0.1.0/test-process-plugin.zip";
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
        environment.set_fs_time(1);
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        let plugin = approval.items[0].specifier.clone();
        // setting it up takes a while
        environment.add_unresponsive_remote_file(ZIP_URL);
        let deny = async {
          while environment.remote_file_request_count(ZIP_URL) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
          }
          // ex. "Always Deny" in another window
          environment.set_fs_time(2);
          let decision = crate::configuration::ProcessPluginDecision::new(
            &environment,
            crate::configuration::ProcessPluginApproval::any_config("/a/dprint.json".to_string(), plugin),
            false,
            &[],
          );
          crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
          environment.remove_unresponsive_remote_file(ZIP_URL);
        };
        let (remaining, _) = tokio::join!(client.approve(&approval.id, ApprovalDecision::Allow), deny);
        // it isn't used even though it was set up
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].denied);
        let result = client.format("/a/file.txt_ps", "text").await;
        assert_eq!(result.not_formatted_reason(), Some(NotFormattedReason::ApprovalRequired));
        assert!(result.approvals()[0].denied);
        // and the allow isn't stored over the denial
        let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].allowed);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_only_forget_the_decisions_of_the_config_file() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      for config_file in ["/a/dprint.json", "/b/dprint.json"] {
        let approval = client.config_info(config_file).await.status.approvals()[0].clone();
        client.approve(&approval.id, ApprovalDecision::Deny).await;
      }
      let params = ForgetDecisionsParams {
        config_file: Some(ConfigFileIdentifier {
          uri: uri("/a/dprint.json"),
          dir_uri: None,
        }),
        work_done_token: None,
      };
      client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
      assert!(!client.config_info("/a/dprint.json").await.status.approvals()[0].denied);
      assert!(client.config_info("/b/dprint.json").await.status.approvals()[0].denied);
    }
    .boxed_local()
  });
}

#[test]
fn should_keep_denying_with_the_approvals_of_a_newer_version_of_dprint() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
      client.approve(&approval.id, ApprovalDecision::AlwaysDeny).await;
    }
    .boxed_local()
  });
  // a newer version of dprint wrote it
  let file_path = crate::configuration::get_editor_approvals_file_path(&environment).unwrap();
  let mut approvals = serde_json::from_str::<serde_json::Value>(&environment.read_file(&file_path).unwrap()).unwrap();
  approvals["minVersion"] = json!(999);
  environment.write_file(&file_path, &approvals.to_string()).unwrap();
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
      assert!(approval.denied);
      assert_eq!(approval.previous_decision, Some(ApprovalDecision::AlwaysDeny));
    }
    .boxed_local()
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
        let status = client.config_info("/dprint.json").await.status;
        assert_eq!(status.reason(), Some(NotFormattedReason::ConfigFileMissing));
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
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::ServerError(-32803));
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
          vec![ApprovalKind::Downloads, ApprovalKind::ProcessPlugins]
        );
        assert_eq!(approvals[0].config_file, local_config("/a/dprint.json"));

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
        .dprint_config_schema(config_params("/dprint.json"), CancellationToken::new())
        .await
        .unwrap();
      assert!(result.complete);
      assert_eq!(result.schema["properties"]["test-plugin"]["$ref"], json!("#/definitions/plugin-test-plugin"));
      // a file that isn't a config file is invalid like for dprint/configInfo
      let mut params = config_params("/dprint.json");
      params.config_file.as_mut().unwrap().uri = uri("/other.json");
      let err = client.backend.dprint_config_schema(params, CancellationToken::new()).await.unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
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
          .dprint_config_schema(config_params("/dprint.json"), CancellationToken::new())
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
          show: ShowPolicy::Command,
          severity: Severity::Warning,
          claim: true,
          actions: Vec::new(),
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
    self.backend.dprint_format(params, CancellationToken::new()).await.unwrap().result
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
      .dprint_config_info(config_params(path_or_uri), CancellationToken::new())
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
      .config_approvals
      .approvals
  }

  async fn pending_approvals(&self) -> Vec<PendingApproval> {
    self
      .backend
      .dprint_pending_approvals(Some(PendingApprovalsParams::default()), CancellationToken::new())
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

  /// Waits for the config changes after dprint's own files changed, which
  /// the server handles a little after they're watched to change.
  async fn wait_for_dprint_files_change(&self) -> Vec<Uri> {
    for _ in 0..100 {
      let changes = self.take_config_changes();
      if !changes.is_empty() {
        return changes;
      }
      tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
  run_session_with(environment, editor_extension_params(), action);
}

/// Runs a session of an editor extension that's initialized with the params.
fn run_session_with(environment: &TestEnvironment, params: InitializeParams, action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + 'static) {
  run_session_with_config(environment, params, None, action);
}

/// Runs a session of an editor extension of a server that was started with
/// the config file (`dprint lsp --config <path>`).
fn run_session_with_config(
  environment: &TestEnvironment,
  params: InitializeParams,
  config_override: Option<PathBuf>,
  action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + 'static,
) {
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend_with_config(environment.clone(), config_override);
    let backend = Rc::new(backend);
    let run_test_task = dprint_core::async_runtime::spawn({
      let test_client = test_client.clone();
      async move {
        initialize_backend(&backend, params).await;
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
    file_uri: None,
    global_config: None,
    work_done_token: None,
  }
}

fn formatted(text: &str, formatted_text: &str) -> FormatResult {
  let line_index = LineIndex::new(text);
  FormatResult::Formatted {
    edits: to_lsp_edits(crate::commands::lsp::text::get_utf16_edits(text, formatted_text), &line_index),
    notice: None,
  }
}

fn not_formatted(reason: NotFormattedReason, message: Option<&str>) -> FormatResult {
  FormatResult::NotFormatted(NotFormatted::new(reason, message.map(|message| message.to_string()), Vec::new()))
}

fn not_formatted_status(reason: NotFormattedReason, message: Option<&str>) -> FormatStatus {
  FormatStatus::NotFormatted(NotFormatted::new(reason, message.map(|message| message.to_string()), Vec::new()))
}

fn specifiers(approval: &PendingApproval) -> Vec<&str> {
  approval.items.iter().map(|item| item.specifier.as_str()).collect()
}

/// Gets what a status says for the assertions.
trait StatusExt {
  fn reason(&self) -> Option<NotFormattedReason>;
  fn message(&self) -> Option<&str>;
  fn approvals(&self) -> &[PendingApproval];
  fn claim(&self) -> bool;
}

impl StatusExt for FormatStatus {
  fn claim(&self) -> bool {
    match self {
      FormatStatus::Ready => true,
      FormatStatus::NotFormatted(not_formatted) => not_formatted.claim,
    }
  }

  fn reason(&self) -> Option<NotFormattedReason> {
    match self {
      FormatStatus::NotFormatted(not_formatted) => Some(not_formatted.reason),
      FormatStatus::Ready => None,
    }
  }

  fn message(&self) -> Option<&str> {
    match self {
      FormatStatus::NotFormatted(not_formatted) => Some(&not_formatted.message),
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
    dir_uri: uri(if dir_path.is_empty() { "/" } else { dir_path }),
    kind: ConfigFileKind::Local,
    // the workspace folder is the root directory
    in_workspace: true,
  }
}

/// Gets the params for a config file, or a document when it isn't a config file's path.
fn config_params(path_or_uri: &str) -> ConfigParams {
  let identifier = DocumentIdentifier { uri: uri(path_or_uri) };
  let is_config_file = path_or_uri.ends_with("dprint.json") || path_or_uri.ends_with("dprint.jsonc");
  ConfigParams {
    config_file: is_config_file.then(|| ConfigFileIdentifier {
      uri: identifier.uri.clone(),
      dir_uri: None,
    }),
    text_document: (!is_config_file).then_some(identifier),
    language: None,
    notebook_document: None,
    file_uri: None,
    global_config: None,
    work_done_token: None,
  }
}

fn file_pattern(glob: &str, config_keys: &[&str]) -> FilePattern {
  FilePattern {
    glob: glob.to_string(),
    config_keys: config_keys.iter().map(|key| key.to_string()).collect(),
  }
}

fn uri(path_or_uri: &str) -> Uri {
  if path_or_uri.starts_with('/') {
    Uri::from_str(&format!("file://{}", path_or_uri)).unwrap()
  } else {
    Uri::from_str(path_or_uri).unwrap()
  }
}
