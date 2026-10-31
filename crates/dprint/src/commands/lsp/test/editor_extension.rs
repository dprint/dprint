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
use crate::commands::lsp::workspace::messages::Decision;
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
      // where `dprint init --global` creates the global config file, which is used over a `dprint.json`
      let global_config_uri = capability["files"]["globalConfigUri"].as_str().unwrap();
      assert!(global_config_uri.ends_with("/dprint/dprint.jsonc"), "{}", global_config_uri);
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

  // the config file a server was started with is the only one, so there are no names to look for
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/config/my-config.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.clone().run_in_runtime(async move {
    let (backend, recv_task, test_client) = setup_backend_with_config(environment.clone(), Some(PathBuf::from("/config/my-config.json")));
    let run_test_task = dprint_core::async_runtime::spawn(async move {
      let result = initialize_backend(&backend, editor_extension_params()).await;
      let capability = &result.capabilities.experimental.unwrap()["dprint"];
      assert_eq!(capability["configFileGlobs"], json!([]));
      assert_eq!(capability["configFileNames"], json!([]));
      assert_eq!(capability["dotConfigFileNames"], json!([]));
      backend.shutdown().await.unwrap();
    });
    try_join!(recv_task, run_test_task).unwrap();
    test_client.take_messages();
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
    // why the dprint methods aren't used is logged
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
    assert_eq!(
      response["result"]["capabilities"]["experimental"]["dprint"]["configFileNames"],
      json!(["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"])
    );
    assert_eq!(
      response["result"]["capabilities"]["experimental"]["dprint"]["dotConfigFileNames"],
      json!(["dprint.json", "dprint.jsonc"])
    );
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
        r#"{ "associations": ["*.foo", "/src/*.bar", "./b/*.baz", "../c/*.qux", "!*.foo", "**/*.{yml,yaml}", "d/{e,f/*}.{g,h}", "[^_]*.md", "{a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}{o,p}{q,r}.x"] }"#,
      );
    })
    .initialize()
    .build();
  run_session(&environment, |client| {
    async move {
      let config_info = client.config_info("/a/dprint.json").await;
      assert_eq!(
        config_info.file_patterns.iter().map(|pattern| pattern.glob.as_str()).collect::<Vec<_>>(),
        vec![
          "**/*.[tT][xX][tT]",
          "**/*.foo",
          "src/*.bar",
          "b/*.baz",
          // without braces or a `^` class, which an editor's globs may not have
          "**/*.yml",
          "**/*.yaml",
          "d/e.g",
          "d/e.h",
          "d/f/*.g",
          "d/f/*.h",
          "**/[!_]*.md",
        ]
      );
      // what can't be made relative to the directory or has too many globs
      assert_eq!(
        config_info.other_files,
        vec![OtherFiles {
          kind: OtherFilesKind::Associations,
          values: vec!["../c/*.qux".to_string(), "{a,b}{c,d}{e,f}{g,h}{i,j}{k,l}{m,n}{o,p}{q,r}.x".to_string()],
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
fn should_resolve_the_global_config_file_again_when_what_it_extends_changes() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_global_config(|c| {
      c.add_remote_wasm_plugin().add_config_section("extends", r#""./base.json""#);
    })
    .build();
  let global_config_path = crate::configuration::resolve_global_config_path_and_text(&environment)
    .unwrap()
    .unwrap()
    .source
    .display();
  let base_path = Path::new(&global_config_path).parent().unwrap().join("base.json");
  environment.write_file(&base_path, "{}").unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let global_uri = file_path_to_uri(Path::new(&global_config_path)).unwrap();
        let mut params = config_params("/dprint.json");
        params.config_file = Some(ConfigFileIdentifier {
          uri: global_uri.clone(),
          dir_uri: None,
        });
        let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
        assert_eq!(config_info.status, FormatStatus::ready());
        client.take_config_changes();
        // what it extends changed, which the editor reports because it's in its `watchUris`
        environment.write_file(&base_path, r#"{ "test-plugin": { "ending": "changed" } }"#).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: file_path_to_uri(&base_path).unwrap(),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![global_uri]);
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
fn should_provide_the_plugins_known_before_for_the_same_config_file_when_they_need_to_be_approved() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_process_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "file_extensions": ["py"] }"#);
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "file_extensions": ["rs"] }"#);
    })
    .with_local_config("/c/dprint.json", |c| {
      c.add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      // the plugin matches other files for each config file because of its configuration
      for path in ["/a/dprint.json", "/b/dprint.json"] {
        let approval = client.config_info(path).await.status.approvals()[0].clone();
        assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
        assert_eq!(client.config_info(path).await.status, FormatStatus::ready());
      }
    }
    .boxed_local()
  });
  run_session(&environment, |client| {
    async move {
      for (path, extension) in [("/a/dprint.json", "py"), ("/b/dprint.json", "rs")] {
        let config_info = client.config_info(path).await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
        assert_eq!(config_info.plugins.unwrap()[0].file_extensions, vec![extension.to_string()]);
      }
      // what it matches isn't known for a config file it wasn't used with
      let config_info = client.config_info("/c/dprint.json").await;
      assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ApprovalRequired));
      assert_eq!(config_info.plugins, None);
      assert_eq!(config_info.file_patterns, Vec::new());
    }
    .boxed_local()
  });
}

#[test]
fn should_only_claim_the_documents_the_last_plugins_formatted_while_a_config_file_has_an_error() {
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
        assert!(client.config_info("/dprint.json").await.plugins.is_some());
        // ex. a typo
        environment.write_file("/dprint.json", "{ nope").unwrap();
        let status = client.file_info("/file.txt").await.status;
        assert_eq!(status.reason(), Some(NotFormattedReason::ConfigError));
        assert!(status.claim());
        // the plugins it had before didn't format this, so the editor's other formatters are used
        let status = client.file_info("/README.md").await.status;
        assert_eq!(status.reason(), Some(NotFormattedReason::ConfigError));
        assert!(!status.claim());
        let result = client.format("/README.md", "text").await;
        assert_eq!(result.not_formatted_reason(), Some(NotFormattedReason::ConfigError));
        assert!(!result.claim());
        assert!(client.format("/file.txt", "text").await.claim());
      }
      .boxed_local()
    }
  });
  // what the plugins formatted isn't known in a new session
  run_session(&environment, |client| {
    async move {
      let status = client.file_info("/file.txt").await.status;
      assert_eq!(status.reason(), Some(NotFormattedReason::ConfigError));
      assert!(!status.claim());
      assert_eq!(client.config_info("/dprint.json").await.plugins, None);
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
fn should_stop_watching_a_config_file_that_is_on_another_machine_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "watchFiles": true } } }));
  params.capabilities.workspace = Some(WorkspaceClientCapabilities {
    did_change_watched_files: Some(deno_tower_lsp::lsp_types::DidChangeWatchedFilesClientCapabilities {
      dynamic_registration: Some(true),
      relative_pattern_support: Some(true),
    }),
    ..Default::default()
  });
  run_session_with(&environment, params, {
    let environment = environment.clone();
    move |client| {
      async move {
        let registration_ids = || client.test_client.registrations.lock().iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
        assert!(registration_ids().contains(&"dprint-watched-files".to_string()));
        // ex. a `git pull` replaced it with a symlink to a share
        environment.remove_file("/a/dprint.json").unwrap();
        environment.symlink(network_path("dprint.json"), "/a/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/a/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        assert!(client.config_info("/a/dprint.json").await.status.approvals()[0].config_file.on_another_machine);
        // so the editor doesn't watch it anymore, which would follow the symlink
        assert_eq!(registration_ids(), vec!["dprint-config-file-watcher".to_string()]);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_watch_a_directory_of_an_extended_file_that_is_on_another_machine_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("extends", r#""./shared/base.json""#);
    })
    .build();
  environment.mk_dir_all("/a/shared").unwrap();
  environment.write_file("/a/shared/base.json", "{}").unwrap();
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "watchFiles": true } } }));
  params.capabilities.workspace = Some(WorkspaceClientCapabilities {
    did_change_watched_files: Some(deno_tower_lsp::lsp_types::DidChangeWatchedFilesClientCapabilities {
      dynamic_registration: Some(true),
      relative_pattern_support: Some(true),
    }),
    ..Default::default()
  });
  run_session_with(&environment, params, {
    let environment = environment.clone();
    move |client| {
      async move {
        let watched_dirs = || {
          let registrations = client.test_client.registrations.lock();
          let Some(registration) = registrations.iter().find(|r| r.id == "dprint-watched-files") else {
            return Vec::new();
          };
          registration.register_options.as_ref().unwrap()["watchers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|watcher| watcher["globPattern"]["baseUri"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
        };
        let config_info = client.config_info("/a/dprint.json").await;
        assert_eq!(config_info.status, FormatStatus::ready());
        assert_eq!(config_info.watch_uris, vec![uri("/a/shared/base.json")]);
        assert!(watched_dirs().contains(&uri("/a/shared").as_str().to_string()));
        // ex. a `git pull` replaced the directory with a symlink to a share
        environment.remove_dir_all("/a/shared").unwrap();
        environment.symlink(network_path("shared"), "/a/shared").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/a/shared/base.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        let config_info = client.config_info("/a/dprint.json").await;
        assert_eq!(config_info.status.approvals()[0].items[0].kind, ApprovalItemKind::NetworkPath);
        // so neither the editor nor the server watches the directory, which would follow it
        assert_eq!(config_info.watch_uris, Vec::<Uri>::new());
        assert!(!watched_dirs().contains(&uri("/a/shared").as_str().to_string()), "{:?}", watched_dirs());
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_check_a_directory_alias_again_before_providing_a_path_through_it() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/libs/shared/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/repo/apps/web/shared", "/repo/libs/shared");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // a document opened through the symlink makes it an alias of the directory it points to, so the config
        // file is provided through it for a document opened by its canonical path too
        client.file_info("/repo/apps/web/shared/sub/file.txt").await;
        let config_file = client.file_info("/repo/libs/shared/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/repo/apps/web/shared/dprint.json"));
        // it points somewhere else now (ex. a `git pull` changed it), which the editor doesn't report, so it
        // isn't an alias for the next request
        environment.remove_canonical_path("/repo/apps/web/shared");
        environment.add_canonical_path("/repo/apps/web/shared", "/repo/other");
        let config_file = client.file_info("/repo/libs/shared/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/repo/libs/shared/dprint.json"));
      }
      .boxed_local()
    }
  });
}

/// On a case sensitive file system (ex. Linux), this covers a `.config`
/// directory that's a symlink to another one, which is known by the names
/// that were looked up the same way.
#[test]
fn should_know_a_config_file_by_the_names_that_were_looked_up_when_they_are_in_another_case() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/p/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/p/.Config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a case insensitive file system, which finds `.Config` when `.config` is looked up
  environment.add_canonical_path("/p/.config", "/p/.Config");
  run_session(&environment, |client| {
    async move {
      let config_file = client.file_info("/p/file.txt").await.config_file.unwrap();
      // so it's used for the directory it's found for
      assert_eq!(config_file, local_config("/p/.config/dprint.json"));
      let mut params = config_params("/p/.config/dprint.json");
      params.config_file = Some(ConfigFileIdentifier {
        uri: config_file.uri.clone(),
        dir_uri: Some(config_file.dir_uri.clone()),
      });
      let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
      assert_eq!(config_info.status, FormatStatus::ready());
      assert_eq!(config_info.config_file, Some(local_config("/p/.config/dprint.json")));
    }
    .boxed_local()
  });
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn should_know_a_document_in_a_dot_config_dir_in_another_case_by_the_config_file_of_its_parent() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/p/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/p/.Config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/q/Dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a case insensitive file system, which finds `.Config` and `Dprint.json` when the names dprint looks up are
  environment.add_canonical_path("/p/.config", "/p/.Config");
  environment.add_canonical_path("/q/dprint.json", "/q/Dprint.json");
  run_session(&environment, |client| {
    async move {
      // the config file itself when it's opened, which is the same config file as for its parent
      let config_file = client.file_info("/p/.Config/dprint.json").await.config_file.unwrap();
      assert_eq!(config_file, local_config("/p/.config/dprint.json"));
      // where a name that isn't found in a `.config` directory isn't a config file
      let err = client
        .backend
        .dprint_config_info(config_params("/p/.Config/.dprint.json"), CancellationToken::new())
        .await
        .unwrap_err();
      assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
      // and the editor's path of a config file whose name is in another case is accepted
      let mut params = config_params("/q/file.txt");
      params.text_document = None;
      params.config_file = Some(ConfigFileIdentifier {
        uri: uri("/q/Dprint.json"),
        dir_uri: None,
      });
      let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
      assert_eq!(config_info.status, FormatStatus::ready());
      assert_eq!(config_info.config_file, Some(local_config("/q/dprint.json")));
    }
    .boxed_local()
  });
}

#[test]
fn should_watch_a_config_file_through_a_directory_symlink_the_editor_does_not_watch_through_by_its_canonical_path() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/other/shared/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/repo/packages/shared", "/other/shared");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // a document opened through the symlink, which isn't a workspace folder or one of its ancestors
        let config_file = client.file_info("/repo/packages/shared/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/repo/packages/shared/dprint.json"));
        let config_info = client.config_info("/repo/packages/shared/dprint.json").await;
        assert_eq!(config_info.watch_uris, vec![uri("/other/shared/dprint.json")]);
        // so the editor reports a change there, which is a change of the config file
        let text = environment.read_file("/other/shared/dprint.json").unwrap();
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/other/shared/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/other/shared/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        let mut changes = client.wait_for_config_changes().await;
        changes.dedup();
        assert_eq!(changes, vec![uri("/repo/packages/shared/dprint.json")]);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_use_a_config_file_the_server_was_started_with_that_is_a_symlink_for_the_directory_it_is_in() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/shared/base.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "shared" }"#);
    })
    .with_local_config("/repo/packages/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("test-plugin", r#"{ "ending": "shared" }"#);
    })
    .build();
  environment.add_canonical_path("/repo/packages/a/dprint.json", "/repo/shared/base.json");
  run_session_with_config(
    &environment,
    editor_extension_params(),
    Some(PathBuf::from("/repo/packages/a/dprint.json")),
    |client| {
      async move {
        // like `dprint fmt --config`
        let file_info = client.file_info("/repo/packages/a/file.txt").await;
        assert_eq!(file_info.status, FormatStatus::ready());
        let config_file = file_info.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/repo/packages/a/dprint.json"));
        assert_eq!(config_file.dir_uri, uri("/repo/packages/a"));
        assert_eq!(client.format("/repo/packages/a/file.txt", "text").await, formatted("text", "text_shared"));
      }
      .boxed_local()
    },
  );
}

#[test]
fn should_take_back_the_diagnostics_of_a_config_file_in_a_symlinked_dot_config_dir_that_is_on_another_machine_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/x/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/p/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a symlink to a local directory, which is read through it
  environment.add_canonical_path("/p/.config", "/x");
  take_back_diagnostics_of_config_file_on_another_machine(&environment, "/p/file.txt", "/p/.config/dprint.json");
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn should_take_back_the_diagnostics_of_a_config_file_in_a_dot_config_dir_in_another_case_that_is_on_another_machine_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/p/.Config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/p/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a case insensitive file system
  environment.add_canonical_path("/p/.config", "/p/.Config");
  take_back_diagnostics_of_config_file_on_another_machine(&environment, "/p/file.txt", "/p/.config/dprint.json");
}

/// Publishes the diagnostics of the config file found for the document, which
/// is then replaced with a symlink to a share, and checks that they're taken
/// back and that the editor is told it changed.
fn take_back_diagnostics_of_config_file_on_another_machine(environment: &TestEnvironment, document_path: &'static str, config_path: &'static str) {
  run_session(environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        environment.write_file(config_path, "{").unwrap();
        client.file_info(document_path).await;
        let diagnostics = client.wait_for_published_diagnostics().await;
        assert_eq!(diagnostics[0].0, uri(config_path));
        client.take_config_changes();
        // ex. a `git pull` replaced it with a symlink to a share
        environment.remove_file(config_path).unwrap();
        environment.symlink(network_path("dprint.json"), config_path).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri(config_path),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_published_diagnostics().await, vec![(uri(config_path), Vec::new())]);
        assert!(client.wait_for_config_changes().await.contains(&uri(config_path)));
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_take_back_the_diagnostics_published_through_a_directory_symlink_that_points_elsewhere_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/libs/pkg/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  environment.add_canonical_symlink("/repo/pkg", "/repo/libs/pkg");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // a document opened through the symlink makes it an alias of the directory it points to
        client.file_info("/repo/pkg/file.txt").await;
        // so the config file's problems are published through it once they change
        let text = environment.read_file("/repo/libs/pkg/dprint.json").unwrap();
        environment
          .write_file("/repo/libs/pkg/dprint.json", &text.replacen('{', r#"{ "otherUnknownProperty": 1, "#, 1))
          .unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/repo/libs/pkg/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        client.wait_for_config_changes().await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        let last = diagnostics.last().unwrap();
        assert_eq!(last.0, uri("/repo/pkg/dprint.json"));
        assert_eq!(last.1.len(), 2);
        // it points somewhere else now, so the problems are taken back and published again where the editor knows
        // the config file now
        environment.remove_canonical_path("/repo/pkg");
        environment.add_canonical_path("/repo/pkg", "/repo/other");
        client.file_info("/repo/libs/pkg/file.txt").await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 2, "{:?}", diagnostics);
        assert_eq!(diagnostics[0], (uri("/repo/pkg/dprint.json"), Vec::new()));
        // and an editor extension is told its config file changed there
        assert!(client.take_config_changes().contains(&uri("/repo/pkg/dprint.json")));
        assert_eq!(diagnostics[1].0, uri("/repo/libs/pkg/dprint.json"));
        assert_eq!(diagnostics[1].1.len(), 2);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_tell_about_a_config_file_without_problems_that_was_known_through_an_alias_that_points_elsewhere_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/libs/pkg/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/repo/pkg", "/repo/libs/pkg");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.file_info("/repo/pkg/file.txt").await;
        let config_file = client.file_info("/repo/libs/pkg/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/repo/pkg/dprint.json"));
        client.take_config_changes();
        // it points somewhere else now, so the editor gets the result of what it knew through it again
        environment.remove_canonical_path("/repo/pkg");
        environment.add_canonical_path("/repo/pkg", "/repo/other");
        client.file_info("/repo/libs/pkg/file.txt").await;
        assert!(client.take_config_changes().contains(&uri("/repo/pkg/dprint.json")));
      }
      .boxed_local()
    }
  });
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn should_forget_a_listed_decision_by_its_config_file_in_another_case() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // ex. stored by a version that keeps the case on disk
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: Path::new("/a").join("Dprint.json").display().to_string(),
            plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
            config_hash: Some("1:hash".to_string()),
            config_key: None,
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/a/dprint.json"),
            dir_uri: Some(uri("/a")),
          }),
          work_done_token: None,
        };
        client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
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
fn should_move_the_diagnostics_of_a_directory_to_another_symlink_that_leads_to_it_too() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  environment.add_canonical_symlink("/ws/b", "/ws/real");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // the problems are published through the first symlink
        client.file_info("/ws/a/file.txt").await;
        let text = environment.read_file("/ws/real/dprint.json").unwrap();
        environment
          .write_file("/ws/real/dprint.json", &text.replacen('{', r#"{ "otherUnknownProperty": 1, "#, 1))
          .unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/ws/real/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        client.wait_for_config_changes().await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.last().unwrap().0, uri("/ws/a/dprint.json"));
        // so they're moved to the other one once it's what the editor knows the directory by, which is checked
        client.file_info("/ws/b/file.txt").await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 2, "{:?}", diagnostics);
        assert_eq!(diagnostics[0], (uri("/ws/a/dprint.json"), Vec::new()));
        assert_eq!(diagnostics[1].0, uri("/ws/b/dprint.json"));
        assert_eq!(diagnostics[1].1.len(), 2);
        assert_eq!(client.take_config_changes(), Vec::<Uri>::new());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_take_back_the_diagnostics_of_a_directory_another_symlink_leads_to_too() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  // ex. two symlinks to one package
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  environment.add_canonical_symlink("/ws/b", "/ws/real");
  run_session(&environment, |client| {
    async move {
      client.file_info("/ws/a/file.txt").await;
      client.take_config_changes();
      // the documents of both are used, which each still leads to the same directory
      for path in ["/ws/b/file.txt", "/ws/a/file.txt", "/ws/b/file.txt", "/ws/a/file.txt"] {
        client.file_info(path).await;
        client.config_info(path).await;
        assert_eq!(client.take_config_changes(), Vec::<Uri>::new(), "{}", path);
      }
    }
    .boxed_local()
  });
}

#[test]
fn should_tell_about_a_config_file_known_through_a_replaced_alias_that_points_elsewhere_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  environment.add_canonical_symlink("/ws/b", "/ws/real");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.file_info("/ws/a/file.txt").await;
        client.take_config_changes();
        // the other symlink replaces it as what the editor knows the directory by, which changes nothing
        client.file_info("/ws/b/file.txt").await;
        assert_eq!(client.take_config_changes(), Vec::<Uri>::new());
        // the first one points somewhere else now, which a request through the other one finds, and the config file
        // the editor knew through it is told about, even though the other one still knows it in the same directory
        environment.remove_canonical_path("/ws/a");
        environment.add_canonical_path("/ws/a", "/ws/other");
        client.file_info("/ws/b/file.txt").await;
        let config_changes = client.take_config_changes();
        assert_eq!(
          config_changes.iter().filter(|uri_| **uri_ == uri("/ws/a/dprint.json")).count(),
          1,
          "{:?}",
          config_changes
        );
        // which is only told once
        client.file_info("/ws/b/file.txt").await;
        assert_eq!(client.take_config_changes(), Vec::<Uri>::new());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_tell_about_a_config_file_known_through_a_replaced_alias_after_a_reset() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  environment.add_canonical_symlink("/ws/b", "/ws/real");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.file_info("/ws/a/file.txt").await;
        client.file_info("/ws/b/file.txt").await;
        // the config files are forgotten, and the editor gets the results through the other one again
        client.backend.dprint_reset(Some(ResetParams {}), CancellationToken::new()).await.unwrap();
        client.file_info("/ws/b/file.txt").await;
        client.take_config_changes();
        // so the first one is still told about once it points somewhere else
        environment.remove_canonical_path("/ws/a");
        environment.add_canonical_path("/ws/a", "/ws/other");
        client.file_info("/ws/b/file.txt").await;
        assert!(client.take_config_changes().contains(&uri("/ws/a/dprint.json")));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_the_most_recently_replaced_alias_of_a_directory_that_still_leads_to_it() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  for path in ["/ws/a", "/ws/b", "/ws/c"] {
    environment.add_canonical_symlink(path, "/ws/real");
  }
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        for path in ["/ws/a/file.txt", "/ws/b/file.txt", "/ws/c/file.txt"] {
          client.file_info(path).await;
        }
        // the most recent one points somewhere else now, so the one it replaced is provided, which replaced the
        // first one
        environment.remove_canonical_path("/ws/c");
        environment.add_canonical_path("/ws/c", "/ws/other");
        let config_file = client.file_info("/ws/real/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/ws/b/dprint.json"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_publish_the_diagnostics_of_a_config_file_through_the_path_the_editor_first_used() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  run_session(&environment, |client| {
    async move {
      // not its canonical path, which the editor never opened
      client.file_info("/ws/a/file.txt").await;
      let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
      assert_eq!(diagnostics.len(), 1, "{:?}", diagnostics);
      assert_eq!(diagnostics[0].0, uri("/ws/a/dprint.json"));
    }
    .boxed_local()
  });
}

#[test]
fn should_publish_the_diagnostics_through_a_replaced_alias_that_still_leads_to_the_directory() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  for path in ["/ws/a", "/ws/b", "/ws/c"] {
    environment.add_canonical_symlink(path, "/ws/real");
  }
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        for path in ["/ws/a/file.txt", "/ws/b/file.txt", "/ws/c/file.txt"] {
          client.file_info(path).await;
        }
        // they're published through the first one and moved to each one the editor used after it
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.last().unwrap().0, uri("/ws/c/dprint.json"));
        // the last two point somewhere else now, so the problems are taken back from the most recent one and
        // published through the first one, which still leads to the directory
        for path in ["/ws/b", "/ws/c"] {
          environment.remove_canonical_path(path);
          environment.add_canonical_path(path, "/ws/other");
        }
        let config_file = client.file_info("/ws/real/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/ws/a/dprint.json"));
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.first(), Some(&(uri("/ws/c/dprint.json"), Vec::new())), "{:?}", diagnostics);
        let last = diagnostics.last().unwrap();
        assert_eq!(last.0, uri("/ws/a/dprint.json"));
        assert_eq!(last.1.len(), 1);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_provide_another_alias_of_a_directory_that_still_leads_to_it_after_the_most_recent_one_points_elsewhere() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/ws/real/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_symlink("/ws/a", "/ws/real");
  environment.add_canonical_symlink("/ws/b", "/ws/real");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.file_info("/ws/a/file.txt").await;
        client.file_info("/ws/b/file.txt").await;
        client.take_config_changes();
        // the most recent one points somewhere else now, so the editor gets the result of what it knew through it
        // again, and the other one is what the editor knows the directory by then
        environment.remove_canonical_path("/ws/b");
        environment.add_canonical_path("/ws/b", "/ws/other");
        let config_file = client.file_info("/ws/real/file.txt").await.config_file.unwrap();
        assert_eq!(config_file.uri, uri("/ws/a/dprint.json"));
        assert_eq!(config_file.dir_uri, uri("/ws/a"));
        assert!(client.take_config_changes().contains(&uri("/ws/b/dprint.json")));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_take_back_the_diagnostics_published_through_an_alias_that_is_only_used_by_its_path_after_it_points_elsewhere() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/libs/pkg/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("unknownProperty", "1");
    })
    .initialize()
    .build();
  environment.add_canonical_symlink("/repo/pkg", "/repo/libs/pkg");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        client.file_info("/repo/pkg/file.txt").await;
        let text = environment.read_file("/repo/libs/pkg/dprint.json").unwrap();
        environment
          .write_file("/repo/libs/pkg/dprint.json", &text.replacen('{', r#"{ "otherUnknownProperty": 1, "#, 1))
          .unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/repo/libs/pkg/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        client.wait_for_config_changes().await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.last().unwrap().0, uri("/repo/pkg/dprint.json"));
        // it points somewhere else now, and the editor only uses paths through it
        environment.remove_canonical_path("/repo/pkg");
        environment.add_canonical_path("/repo/pkg", "/repo/other");
        client.file_info("/repo/pkg/file.txt").await;
        let diagnostics = client.test_client.published_diagnostics.lock().drain(..).collect::<Vec<_>>();
        assert_eq!(diagnostics.first(), Some(&(uri("/repo/pkg/dprint.json"), Vec::new())), "{:?}", diagnostics);
        // and an editor extension is told its config file changed, so it gets its result again
        assert!(client.take_config_changes().contains(&uri("/repo/pkg/dprint.json")));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_watch_what_a_config_file_that_is_a_dangling_symlink_points_to_without_resolving_it_before() {
  let environment = TestEnvironmentBuilder::new().add_remote_wasm_plugin().build();
  environment.mk_dir_all("/a").unwrap();
  environment.mk_dir_all("/shared").unwrap();
  // ex. a symlink, whose target was deleted while the editor was closed
  environment.add_canonical_path("/a/dprint.json", "/shared/base.json");
  run_session(&environment, |client| {
    async move {
      let config_info = client.config_info("/a/dprint.json").await;
      assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ConfigFileMissing));
      assert_eq!(config_info.watch_uris, vec![uri("/shared/base.json")]);
    }
    .boxed_local()
  });
}

#[test]
fn should_answer_with_a_config_file_that_is_on_another_machine_after_formatting() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.file_info("/a/file.txt").await.status, FormatStatus::ready());
        // ex. a `git pull` replaced it with a symlink to a share after it was resolved, which isn't reported yet
        environment.remove_file("/a/dprint.json").unwrap();
        environment.symlink(network_path("dprint.json"), "/a/dprint.json").unwrap();
        let config_file = client
          .backend
          .dprint_format(client.format_params("/a/file.txt", "text"), CancellationToken::new())
          .await
          .unwrap()
          .config_file
          .unwrap();
        assert!(config_file.on_another_machine);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_answer_with_a_config_file_that_went_on_another_machine_while_formatting() {
  const PLUGIN_URL: &str = "https://plugins.dprint.dev/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build(); // not initialized, so the plugin is downloaded while it formats
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        environment.add_unresponsive_remote_file(PLUGIN_URL);
        let swap = async {
          while environment.remote_file_request_count(PLUGIN_URL) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
          }
          // ex. a `git pull` replaced it with a symlink to a share while it was formatting
          environment.remove_file("/a/dprint.json").unwrap();
          environment.symlink(network_path("dprint.json"), "/a/dprint.json").unwrap();
          environment.remove_unresponsive_remote_file(PLUGIN_URL);
        };
        let (result, _) = tokio::join!(
          client
            .backend
            .dprint_format(client.format_params("/a/file.txt", "text"), CancellationToken::new()),
          swap
        );
        // so the editor doesn't open it from the result
        assert!(result.unwrap().config_file.unwrap().on_another_machine);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_publish_the_error_of_a_config_file_that_was_deleted_while_it_was_resolved() {
  const PLUGIN_URL: &str = "https://plugins.dprint.dev/missing.wasm";
  let environment = TestEnvironmentBuilder::new().build();
  environment.mk_dir_all("/a").unwrap();
  environment
    .write_file("/a/dprint.json", &format!(r#"{{ "plugins": ["{}"] }}"#, PLUGIN_URL))
    .unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        environment.add_unresponsive_remote_file(PLUGIN_URL);
        let delete = async {
          while environment.remote_file_request_count(PLUGIN_URL) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
          }
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
          client.wait_for_config_changes().await;
          client.test_client.published_diagnostics.lock().clear();
          // the plugin then fails to download, which is an error resolving it
          environment.remove_unresponsive_remote_file(PLUGIN_URL);
        };
        let (result, _) = tokio::join!(client.config_info("/a/dprint.json"), delete);
        assert_eq!(result.status.reason(), Some(NotFormattedReason::ConfigError));
        assert_eq!(*client.test_client.published_diagnostics.lock(), Vec::new());
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn should_not_know_two_directories_whose_names_are_only_different_in_case_as_one() {
  // ex. a case sensitive volume, where `.Config` and `.config` are two directories
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/p/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/p/.Config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  run_session(&environment, |client| {
    async move {
      assert_eq!(client.file_info("/p/file.txt").await.config_file, Some(local_config("/p/.config/dprint.json")));
      let config_file = client.file_info("/p/.Config/file.txt").await.config_file.unwrap();
      assert_eq!(config_file.uri, uri("/p/.Config/dprint.json"));
      assert_eq!(config_file.dir_uri, uri("/p/.Config"));
    }
    .boxed_local()
  });
}

#[test]
fn should_take_back_the_diagnostics_of_a_config_file_that_is_on_another_machine_now() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        environment.write_file("/a/dprint.json", "{").unwrap();
        client.config_info("/a/dprint.json").await;
        let diagnostics = client.wait_for_published_diagnostics().await;
        assert_eq!(diagnostics[0].0, uri("/a/dprint.json"));
        assert_eq!(diagnostics[0].1.len(), 1);
        // ex. a `git pull` replaced it with a symlink to a share, so the editor shouldn't open it from them
        environment.remove_file("/a/dprint.json").unwrap();
        environment.symlink(network_path("dprint.json"), "/a/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/a/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_published_diagnostics().await, vec![(uri("/a/dprint.json"), Vec::new())]);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_forget_a_decision_by_the_config_file_it_is_listed_with_after_the_project_moved() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/home/u/code/proj/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // the project was moved, and its old path is a symlink to it
  environment.add_canonical_symlink("/home/u/proj", "/home/u/code/proj");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // ex. stored by running `dprint fmt` before it was moved
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: Path::new("/home/u/proj").join("dprint.json").display().to_string(),
            plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
            config_hash: Some("1:hash".to_string()),
            config_key: None,
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        let decisions = client.decisions().await;
        let listed = decisions.iter().find(|decision| decision.always).unwrap().config_file.clone().unwrap();
        assert_eq!(listed.uri, uri("/home/u/proj/dprint.json"));
        // which is forgotten by what it's listed with, though that's another path of the config file now
        let params = ForgetDecisionsParams {
          config_file: Some(listed),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(forgotten.decisions.len(), 1);
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
fn should_watch_the_config_files_in_any_case_where_the_file_system_finds_them() {
  assert_eq!(
    crate::commands::lsp::get_config_file_glob(false),
    "**/{dprint.json,dprint.jsonc,.dprint.json,.dprint.jsonc}"
  );
  assert_eq!(
    crate::commands::lsp::get_config_file_glob(true),
    "**/{[dD][pP][rR][iI][nN][tT].[jJ][sS][oO][nN],[dD][pP][rR][iI][nN][tT].[jJ][sS][oO][nN][cC],.[dD][pP][rR][iI][nN][tT].[jJ][sS][oO][nN],.[dD][pP][rR][iI][nN][tT].[jJ][sS][oO][nN][cC]}"
  );
}

#[test]
fn should_forget_the_decisions_of_another_config_file_than_the_one_the_server_was_started_with() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  run_session_with_config(&environment, editor_extension_params(), Some(PathBuf::from("/repo/dprint.json")), {
    let environment = environment.clone();
    move |client| {
      async move {
        // ex. stored for another project
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: Path::new("/other").join("dprint.json").display().to_string(),
            plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
            config_hash: Some("1:hash".to_string()),
            config_key: None,
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        // and one of the config file the server was started with, which is kept
        let own_config_file = environment.canonicalize("/repo/dprint.json").unwrap().display().to_string();
        let decision = crate::configuration::ProcessPluginDecision::new(
          &environment,
          crate::configuration::ProcessPluginApproval {
            config_file: own_config_file.clone(),
            plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
            config_hash: Some("1:hash".to_string()),
            config_key: None,
          },
          true,
          &[],
        );
        crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
        let other_uri = uri("/other/dprint.json");
        let listed = client
          .decisions()
          .await
          .into_iter()
          .filter_map(|decision| decision.config_file)
          .find(|config_file| config_file.uri == other_uri)
          .unwrap();
        let params = ForgetDecisionsParams {
          config_file: Some(listed),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(forgotten.decisions.len(), 1);
        let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert_eq!(
          decisions.iter().map(|decision| decision.approval.config_file.clone()).collect::<Vec<_>>(),
          vec![own_config_file]
        );
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_list_the_denial_of_the_config_file_a_symlinked_dot_config_dir_points_to_by_its_own_path() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/repo/.config/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .with_local_config("/repo/packages/a/.config/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  environment.add_canonical_path("/repo/packages/a/.config", "/repo/.config");
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/repo/.config/dprint.json").await.status.approvals()[0].clone();
      client.approve(&approval.id, ApprovalDecision::Deny).await;
      // the server learns the package's path of the directory it points to
      client
        .backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
          changes: vec![FileEvent {
            uri: uri("/repo/packages/a/.config/dprint.json"),
            typ: FileChangeType::CHANGED,
          }],
        })
        .await;
      client.wait_for_config_changes().await;
      // which doesn't make the root's config file known by the package's path
      let decisions = client.decisions().await;
      let denial = decisions.iter().find(|decision| !decision.allows).unwrap();
      let listed = denial.config_file.clone().unwrap();
      assert_eq!(listed.uri, uri("/repo/.config/dprint.json"));
      let params = ForgetDecisionsParams {
        config_file: Some(listed),
        work_done_token: None,
      };
      client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
      assert!(!client.decisions().await.iter().any(|decision| !decision.allows));
    }
    .boxed_local()
  });
}

#[test]
fn should_reject_another_symlink_to_the_config_file_the_server_was_started_with() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/shared/base.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/packages/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/packages/b/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_path("/repo/packages/a/dprint.json", "/repo/shared/base.json");
  environment.add_canonical_path("/repo/packages/b/dprint.json", "/repo/shared/base.json");
  run_session_with_config(
    &environment,
    editor_extension_params(),
    Some(PathBuf::from("/repo/packages/a/dprint.json")),
    |client| {
      async move {
        assert_eq!(client.config_info("/repo/packages/a/dprint.json").await.status, FormatStatus::ready());
        // it's another config file, though it's a symlink to the same file
        let err = client
          .backend
          .dprint_config_info(config_params("/repo/packages/b/dprint.json"), CancellationToken::new())
          .await
          .unwrap_err();
        assert_eq!(err.code, deno_tower_lsp::jsonrpc::ErrorCode::InvalidParams);
      }
      .boxed_local()
    },
  );
}

#[test]
fn should_not_forget_the_decisions_of_the_config_file_a_symlinked_dot_config_dir_points_into() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/packages/a/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a symlink to the `.config` directory of the repository, which is read through it
  environment.add_canonical_path("/repo/packages/a/.config", "/repo/.config");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let store = |config_file: String| {
          let environment = environment.clone();
          async move {
            let decision = crate::configuration::ProcessPluginDecision::new(
              &environment,
              crate::configuration::ProcessPluginApproval {
                config_file,
                plugin: "https://plugins.dprint.dev/test-process.json@checksum".to_string(),
                config_hash: Some("1:hash".to_string()),
                config_key: None,
              },
              true,
              &[],
            );
            crate::configuration::store_process_plugin_decision(&environment, decision).await.unwrap();
          }
        };
        let root_config_file = environment.canonicalize("/repo/.config/dprint.json").unwrap().display().to_string();
        let package_config_file = environment
          .canonicalize("/repo/packages/a")
          .unwrap()
          .join(".config")
          .join("dprint.json")
          .display()
          .to_string();
        store(root_config_file.clone()).await;
        store(package_config_file).await;
        // which the editor may send without the directory it's used for, because it found it
        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/repo/packages/a/.config/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(forgotten.decisions.len(), 1);
        // it's another config file than the one its directory points into
        let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert_eq!(
          decisions.iter().map(|decision| decision.approval.config_file.clone()).collect::<Vec<_>>(),
          vec![root_config_file]
        );
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_tell_the_user_a_denial_of_what_was_allowed_since_is_not_kept_after_its_approval_was_replaced() {
  const PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(PLUGIN_URL)
    .with_local_config("/a/dprint.json", |c| {
      c.add_plugin(PLUGIN_URL);
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_plugin(PLUGIN_URL);
    })
    .build();
  run_session(&environment, |client| {
    async move {
      let a_approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
      let b_approval = client.config_info("/b/dprint.json").await.status.approvals()[0].clone();
      client.approve(&b_approval.id, ApprovalDecision::Allow).await;
      // which resolves the other config file again before the user denies it there
      assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
      let result = client
        .backend
        .dprint_approve(
          ApproveParams {
            id: a_approval.id,
            decision: ApprovalDecision::Deny,
            work_done_token: None,
          },
          CancellationToken::new(),
        )
        .await
        .unwrap();
      let notice = result.notice.unwrap();
      assert!(notice.message.starts_with("What you denied was allowed since"), "{}", notice.message);
      assert_eq!(result.config_approvals.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_watch_a_config_file_in_a_dot_config_dir_that_is_a_symlink_by_what_it_points_to() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/proj/shared-cfg/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/proj/.config/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a symlink, which is read through it
  environment.add_canonical_path("/proj/.config", "/proj/shared-cfg");
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "watchFiles": true } } }));
  params.capabilities.workspace = Some(WorkspaceClientCapabilities {
    did_change_watched_files: Some(deno_tower_lsp::lsp_types::DidChangeWatchedFilesClientCapabilities {
      dynamic_registration: Some(true),
      relative_pattern_support: Some(true),
    }),
    ..Default::default()
  });
  run_session_with(&environment, params, {
    let environment = environment.clone();
    move |client| {
      async move {
        let config_file = client.file_info("/proj/file.txt").await.config_file.unwrap();
        assert_eq!(config_file, local_config("/proj/.config/dprint.json"));
        // the editor doesn't watch through the symlink, which isn't a workspace folder, so it's what it points to
        let watch_uris = client.config_info("/proj/.config/dprint.json").await.watch_uris;
        assert_eq!(watch_uris, vec![uri("/proj/shared-cfg/dprint.json")]);
        // which a change of it doesn't change, though the server canonicalized a path through the symlink then
        let text = environment.read_file("/proj/shared-cfg/dprint.json").unwrap();
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/proj/shared-cfg/dprint.json", &text).unwrap();
        environment.write_file("/proj/.config/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/proj/shared-cfg/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        let mut changes = client.wait_for_config_changes().await;
        changes.dedup();
        assert_eq!(changes, vec![uri("/proj/.config/dprint.json")]);
        let watch_uris = client.config_info("/proj/.config/dprint.json").await.watch_uris;
        assert_eq!(watch_uris, vec![uri("/proj/shared-cfg/dprint.json")]);
        // and the registered directories are canonical, so none is the symlink
        let base_uris = {
          let registrations = client.test_client.registrations.lock();
          let registration = registrations.iter().rev().find(|r| r.id == "dprint-watched-files").unwrap();
          registration.register_options.as_ref().unwrap()["watchers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|watcher| watcher["globPattern"]["baseUri"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
        };
        assert_eq!(base_uris, vec![uri("/proj/shared-cfg").as_str().to_string()]);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_register_the_files_to_watch_by_their_canonical_directories() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/data/proj/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("extends", r#""./base.json""#);
    })
    .build();
  environment.write_file("/data/proj/base.json", "{}").unwrap();
  // the workspace is opened through a symlink
  environment.add_canonical_symlink("/home/proj", "/data/proj");
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
    uri: uri("/home/proj"),
    name: "proj".to_string(),
  }]);
  run_session_with(&environment, params, {
    let environment = environment.clone();
    move |client| {
      async move {
        // a document the editor opened in it is what tells the server the editor's paths
        client.file_info("/home/proj/file.txt").await;
        let config_info = client.config_info("/home/proj/dprint.json").await;
        assert_eq!(config_info.status, FormatStatus::ready());
        // what the editor watches itself is by its paths
        assert_eq!(config_info.watch_uris, vec![uri("/home/proj/base.json")]);
        // while the registration is of the canonical directories, which aren't symlinks
        let base_uris = {
          let registrations = client.test_client.registrations.lock();
          let registration = registrations.iter().find(|r| r.id == "dprint-watched-files").unwrap();
          registration.register_options.as_ref().unwrap()["watchers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|watcher| watcher["globPattern"]["baseUri"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
        };
        assert_eq!(base_uris, vec![uri("/data/proj").as_str().to_string(); 2]);
        // so its changes are of the canonical paths, which the editor is told about by its paths
        environment
          .write_file("/data/proj/base.json", r#"{ "test-plugin": { "ending": "changed" } }"#)
          .unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/data/proj/base.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/home/proj/dprint.json")]);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_notice_a_change_to_a_config_file_that_is_a_symlink_to_a_file_with_another_name() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/shared/base.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a symlink, whose canonical path is the file it points to
  environment.add_canonical_path("/a/dprint.json", "/shared/base.json");
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "features": { "watchFiles": true } } }));
  params.capabilities.workspace = Some(WorkspaceClientCapabilities {
    did_change_watched_files: Some(deno_tower_lsp::lsp_types::DidChangeWatchedFilesClientCapabilities {
      dynamic_registration: Some(true),
      relative_pattern_support: Some(true),
    }),
    ..Default::default()
  });
  run_session_with(&environment, params, {
    let environment = environment.clone();
    move |client| {
      async move {
        let file_info = client.file_info("/a/file.txt").await;
        assert_eq!(file_info.status, FormatStatus::ready());
        // it's known by the path it was found at, which is in the directory it's used for
        let config_file = file_info.config_file.unwrap();
        assert_eq!(config_file, local_config("/a/dprint.json"));
        let identifier = ConfigFileIdentifier {
          uri: config_file.uri.clone(),
          dir_uri: Some(config_file.dir_uri.clone()),
        };
        // so the methods that take a config file accept it
        let mut params = config_params("/a/dprint.json");
        params.config_file = Some(identifier.clone());
        let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
        assert_eq!(config_info.status, FormatStatus::ready());
        assert_eq!(config_info.config_file, Some(local_config("/a/dprint.json")));
        // and the editor is told to watch its canonical path, because its watcher of the config files doesn't report it
        assert!(config_info.watch_uris.contains(&uri("/shared/base.json")), "{:?}", config_info.watch_uris);
        let params = PendingApprovalsParams {
          config_file: Some(identifier.clone()),
          work_done_token: None,
        };
        let pending = client.backend.dprint_pending_approvals(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(pending.status, Some(FormatStatus::ready()));
        let params = ForgetDecisionsParams {
          config_file: Some(identifier),
          work_done_token: None,
        };
        assert!(client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.is_ok());
        client.take_config_changes();
        // the change is reported by the canonical path, which isn't the name of a config file
        let text = environment.read_file("/shared/base.json").unwrap();
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/shared/base.json", &text).unwrap();
        // which is what's read through the symlink too
        environment.write_file("/a/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/shared/base.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        // and deleting it is like deleting the config file, which is a dangling symlink then
        environment.remove_file("/shared/base.json").unwrap();
        environment.remove_file("/a/dprint.json").unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/shared/base.json"),
              typ: FileChangeType::DELETED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        // which is still watched by what it points to, so it's noticed when that's created again
        let config_info = client.config_info("/a/dprint.json").await;
        assert_eq!(config_info.status.reason(), Some(NotFormattedReason::ConfigFileMissing));
        assert_eq!(config_info.watch_uris, vec![uri("/shared/base.json")]);
        environment.write_file("/shared/base.json", &text).unwrap();
        environment.write_file("/a/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/shared/base.json"),
              typ: FileChangeType::CREATED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
        assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_forget_a_config_file_that_is_a_symlink_when_what_it_pointed_to_is_deleted_while_it_points_elsewhere() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/shared/v1.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.add_canonical_path("/a/dprint.json", "/shared/v1.json");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.file_info("/a/file.txt").await.status, FormatStatus::ready());
        // a checkout points it at another file and deletes the one it pointed to
        let text = environment.read_file("/shared/v1.json").unwrap();
        environment.write_file("/shared/v2.json", &text).unwrap();
        environment.remove_file("/shared/v1.json").unwrap();
        environment.remove_canonical_path("/a/dprint.json");
        environment.add_canonical_path("/a/dprint.json", "/shared/v2.json");
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![
              FileEvent {
                uri: uri("/shared/v1.json"),
                typ: FileChangeType::DELETED,
              },
              FileEvent {
                uri: uri("/a/dprint.json"),
                typ: FileChangeType::CHANGED,
              },
            ],
          })
          .await;
        client.wait_for_config_changes().await;
        // so it's still known, and a change to what it points to now is noticed
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/shared/v2.json", &text).unwrap();
        environment.write_file("/a/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/shared/v2.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/a/dprint.json")]);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_resolve_a_config_file_again_when_the_symlink_it_inherits_from_points_elsewhere() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/configs/v1.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/sub/dprint.json", |c| {
      c.add_config_section("inherit", "true");
    })
    .build();
  environment.add_canonical_path("/repo/dprint.json", "/repo/configs/v1.json");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.format("/repo/sub/file.txt", "text").await, formatted("text", "text_formatted"));
        client.take_config_changes();
        // a checkout points the symlink at another file
        let text = environment.read_file("/repo/configs/v1.json").unwrap();
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/repo/configs/v2.json", &text).unwrap();
        environment.write_file("/repo/dprint.json", &text).unwrap();
        environment.remove_canonical_path("/repo/dprint.json");
        environment.add_canonical_path("/repo/dprint.json", "/repo/configs/v2.json");
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/repo/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        let mut changes = Vec::new();
        for _ in 0..20 {
          changes.extend(client.wait_for_config_changes().await);
          if changes.contains(&uri("/repo/sub/dprint.json")) {
            break;
          }
        }
        assert!(changes.contains(&uri("/repo/sub/dprint.json")), "{:?}", changes);
        assert_eq!(client.format("/repo/sub/file.txt", "text").await, formatted("text", "text_changed"));
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_use_a_config_file_that_is_a_symlink_to_the_global_config_file_for_its_own_directory() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_global_config(|c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  let global_config_file = crate::configuration::resolve_global_config_path_and_text(&environment).unwrap().unwrap();
  environment.mk_dir_all("/repo").unwrap();
  environment.write_file("/repo/dprint.json", &global_config_file.content).unwrap();
  environment.add_canonical_path("/repo/dprint.json", global_config_file.source.display());
  run_session(&environment, |client| {
    async move {
      let config_file = client.file_info("/repo/file.txt").await.config_file.unwrap();
      assert_eq!(config_file, local_config("/repo/dprint.json"));
      // it's the same config file when it's asked about
      let mut params = config_params("/repo/dprint.json");
      params.config_file = Some(ConfigFileIdentifier {
        uri: config_file.uri.clone(),
        dir_uri: Some(config_file.dir_uri.clone()),
      });
      let config_info = client.backend.dprint_config_info(params, CancellationToken::new()).await.unwrap();
      assert_eq!(config_info.config_file, Some(local_config("/repo/dprint.json")));
      assert_eq!(config_info.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_restore_a_path_download_that_another_version_escaped_another_way() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. kept by a version of dprint with another version of Unicode, which escaped what this one doesn't
  let local_path = Path::new("/project").join("pkg");
  let escaped_local_path = local_path.display().to_string().replace("pkg", "p\\u{6b}g");
  let allowed = format!("{} -> {}", escaped_local_path, network_path("pkg"));
  let escaped_other_path = Path::new("/project").join("other").display().to_string().replace("other", "\\u{6f}ther");
  let denied = format!("{} -> {}", escaped_other_path, network_path("other"));
  let session_file_path = get_session_file_path(&environment, SESSION_ID);
  environment.mk_dir_all(session_file_path.parent().unwrap()).unwrap();
  let text = json!({
    "version": 1,
    "allowedDownloads": [{ "download": allowed, "item": { "specifier": "the allowed path", "kind": "networkPath" } }],
    "deniedDownloads": [{
      "configKey": "file:///dprint.json",
      "downloads": [denied],
      "configFile": { "uri": "file:///dprint.json", "dirUri": "file:///" },
      "items": []
    }]
  });
  environment.write_file(&session_file_path, &text.to_string()).unwrap();
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      let decisions = client.decisions().await;
      // what was allowed is the same path, so it's shown with its item
      let allow = decisions.iter().find(|decision| decision.allows).unwrap();
      assert_eq!(allow.specifiers, vec!["the allowed path".to_string()]);
      // and what was denied is shown as this version makes its text
      let denial = decisions.iter().find(|decision| !decision.allows).unwrap();
      assert_eq!(denial.specifiers.len(), 1);
      assert!(!denial.specifiers[0].contains("\\u{"), "{}", denial.specifiers[0]);
      assert!(
        denial.specifiers[0].starts_with(&Path::new("/project").join("other").display().to_string()),
        "{}",
        denial.specifiers[0]
      );
    }
    .boxed_local()
  });
}

#[test]
fn should_identify_a_config_file_that_is_a_symlink_by_the_directory_it_is_used_for() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/repo/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .with_local_config("/repo/packages/a/dprint.json", |c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  // ex. a symlink to the config file of the repository, which is read through it
  environment.add_canonical_path("/repo/packages/a/dprint.json", "/repo/dprint.json");
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // it's another config file than the one it points to, which is used for its own directory
        let file_info = client.file_info("/repo/packages/a/file.txt").await;
        assert_eq!(file_info.status, FormatStatus::ready());
        assert_eq!(file_info.config_file, Some(local_config("/repo/packages/a/dprint.json")));
        assert_eq!(client.file_info("/repo/file.txt").await.config_file, Some(local_config("/repo/dprint.json")));
        let config_info = client.config_info("/repo/packages/a/dprint.json").await;
        assert_eq!(config_info.status, FormatStatus::ready());
        assert_eq!(config_info.config_file, Some(local_config("/repo/packages/a/dprint.json")));
        assert_eq!(config_info.watch_uris, vec![uri("/repo/dprint.json")]);
        assert!(client.config_info("/repo/dprint.json").await.watch_uris.is_empty());
        assert_eq!(client.format("/repo/packages/a/file.txt", "text").await, formatted("text", "text_formatted"));
        client.take_config_changes();

        // a change of the file it points to is a change of both
        let text = environment.read_file("/repo/dprint.json").unwrap();
        let text = text.replacen('{', r#"{ "test-plugin": { "ending": "changed" }, "#, 1);
        environment.write_file("/repo/dprint.json", &text).unwrap();
        environment.write_file("/repo/packages/a/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/repo/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        let mut changes = client.wait_for_config_changes().await;
        for _ in 0..20 {
          if changes.len() > 1 {
            break;
          }
          changes.extend(client.wait_for_config_changes().await);
        }
        changes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        assert_eq!(changes, vec![uri("/repo/dprint.json"), uri("/repo/packages/a/dprint.json")]);
        assert_eq!(client.format("/repo/packages/a/file.txt", "text").await, formatted("text", "text_changed"));

        // replacing the symlink with a file (ex. switching branches) is a change of the same config file
        environment.remove_canonical_path("/repo/packages/a/dprint.json");
        let text = text.replace(r#""ending": "changed""#, r#""ending": "own""#);
        environment.write_file("/repo/packages/a/dprint.json", &text).unwrap();
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/repo/packages/a/dprint.json"),
              typ: FileChangeType::CHANGED,
            }],
          })
          .await;
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/repo/packages/a/dprint.json")]);
        assert_eq!(client.format("/repo/packages/a/file.txt", "text").await, formatted("text", "text_own"));
        assert!(client.config_info("/repo/packages/a/dprint.json").await.watch_uris.is_empty());
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_store_a_decision_for_a_config_file_that_is_a_symlink_by_the_path_it_was_found_at() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/shared/base.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build();
  environment.add_canonical_path("/a/dprint.json", "/shared/base.json");
  disallow_process_plugins(&environment);
  run_session(&environment, |client| {
    async move {
      // what the user is asked about is the config file they know
      let approval = client.file_info("/a/file.txt").await.status.approvals()[0].clone();
      assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
      assert_eq!(approval.config_file, local_config("/a/dprint.json"));
      assert_eq!(client.approve(&approval.id, ApprovalDecision::AlwaysAllow).await, Vec::new());
      let decisions = client.decisions().await;
      assert_eq!(decisions.len(), 1);
      assert_eq!(
        decisions[0].config_file,
        Some(ConfigFileIdentifier {
          uri: uri("/a/dprint.json"),
          dir_uri: Some(uri("/a")),
        })
      );
      assert_eq!(client.format("/a/file.txt_ps", "text").await, formatted("text", "text_formatted_process"));
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
          approval
            .decisions
            .iter()
            .map(|option| (option.decision, option.label.as_str()))
            .collect::<Vec<_>>(),
          vec![(ApprovalDecision::Allow, "Allow"), (ApprovalDecision::Deny, "Deny")]
        );
        assert!(!approval.denied);
        assert!(
          approval
            .message
            .starts_with("dprint needs to download `https://example.com/test-plugin.wasm` from domains that aren't trusted.")
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
fn should_say_why_the_settings_file_is_not_used_when_asking() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  ensure_global_config_dir(&environment);
  let settings_file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
  // ex. a value of a newer version of dprint that trusts less
  environment
    .write_file(&settings_file_path, r#"{ "editor": { "trustedDomains": ["example.com", "!github.com"] } }"#)
    .unwrap();
  run_session(&environment, {
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(approval.kind, ApprovalKind::Downloads);
        // it doesn't say to trust the domain in the settings file, which the user did
        assert!(!approval.message.contains("Add a domain"), "{}", approval.message);
        assert!(
          approval.message.ends_with(&format!(
            "doesn't use its settings file in editors because {} has a value in \"editor.trustedDomains\" that isn't a domain (\"!github.com\"). Update dprint or change the file to use it.",
            settings_file_path.display()
          )),
          "{}",
          approval.message
        );
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_approve_downloads_from_the_default_domains_when_the_settings_file_is_not_used() {
  const PLUGIN_URL: &str = "https://plugins.dprint.dev/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(PLUGIN_URL);
    })
    .build();
  ensure_global_config_dir(&environment);
  let settings_file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
  // ex. a setting of a newer version of dprint that doesn't trust a default domain
  environment
    .write_file(&settings_file_path, r#"{ "editor": { "untrustedDomains": ["plugins.dprint.dev"] } }"#)
    .unwrap();
  run_session(&environment, {
    move |client| {
      async move {
        let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].kind, ApprovalKind::Downloads);
        assert_eq!(approvals[0].items[0].specifier, PLUGIN_URL);
        assert!(
          approvals[0].message.ends_with(&format!(
            "doesn't use its settings file in editors because {} has a property in \"editor\" that this version of dprint doesn't know (editor.untrustedDomains). Update dprint or change the file to use it.",
            settings_file_path.display()
          )),
          "{}",
          approvals[0].message
        );
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
            .starts_with(&format!("The proxy `{}` that `{}` sets isn't trusted", PROXY_SETTING, npmrc_path)),
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
            local_uri: None,
            plugin: None,
            configuration: None,
          }]
        );
        assert!(
          proxy_approval
            .message
            .starts_with(&format!("The proxy `{}` that `{}` sets isn't trusted", PROXY_SETTING, npmrc_path)),
          "{}",
          proxy_approval.message
        );
        assert!(!proxy_approval.message.contains("npm.example.com"), "{}", proxy_approval.message);
        let url_approval = approvals[1].clone();
        assert_eq!(url_approval.kind, ApprovalKind::Downloads);
        assert_eq!(url_approval.title, "Download from domains that aren't trusted");
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

#[cfg(windows)]
#[test]
fn should_approve_reading_a_config_file_on_another_machine() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_default_config(|c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.mk_dir_all("/src").unwrap();
  environment.symlink(r"\\server\share\dprint.json", "/src/dprint.json").unwrap();
  run_session(&environment, |client| {
    async move {
      // the config file in an ancestor directory isn't used instead, which the cli wouldn't
      let result = client.format("/src/file.txt", "text").await;
      assert_eq!(result.not_formatted_reason(), Some(NotFormattedReason::ApprovalRequired));
      let approvals = result.approvals();
      assert_eq!(approvals.len(), 1);
      let approval = &approvals[0];
      assert_eq!(approval.kind, ApprovalKind::Downloads);
      assert_eq!(approval.title, "Read from another machine");
      assert_eq!(approval.config_file.uri, uri("/src/dprint.json"));
      assert_eq!(approval.config_file.dir_uri, uri("/src"));
      // so the editor doesn't open it before it's allowed, which would read it from the other machine
      assert!(approval.config_file.on_another_machine);
      assert!(client.file_info("/src/file.txt").await.config_file.unwrap().on_another_machine);
      assert!(!client.file_info("/file.txt").await.config_file.unwrap().on_another_machine);
      let specifier = format!(r"{} -> \\server\share\dprint.json", Path::new("/src").join("dprint.json").display());
      assert_eq!(
        approval.items,
        vec![ApprovalItem {
          specifier: specifier.clone(),
          kind: ApprovalItemKind::NetworkPath,
          registry: None,
          npmrc_uri: None,
          local_uri: Some(uri("/src/dprint.json")),
          plugin: None,
          configuration: None,
        }]
      );
      assert!(
        approval
          .message
          .starts_with(&format!("dprint needs to read `{}`, which is on another machine.", specifier)),
        "{}",
        approval.message
      );
      assert!(!approval.message.contains("trustedDomains"), "{}", approval.message);
      // the config file in the directory of another file is used
      assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));
    }
    .boxed_local()
  });
}

#[cfg(windows)]
#[test]
fn should_not_read_a_config_file_from_the_editor_on_another_machine_before_it_is_allowed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_default_config(|c| {
      c.add_remote_wasm_plugin();
    })
    .build();
  environment.mk_dir_all("/src/sub").unwrap();
  environment.symlink(r"\\server\share\dprint.json", "/src/dprint.json").unwrap();
  environment.symlink(r"\\server\share\config", "/src/sub/.config").unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let file_info = client.file_info("/src/file.txt").await;
        let config_file = file_info.config_file.clone().unwrap();
        assert_eq!(config_file.uri, uri("/src/dprint.json"));
        let is_specifier =
          |approval: &PendingApproval| approval.items.len() == 1 && approval.items[0].specifier.ends_with(r"dprint.json -> \\server\share\dprint.json");

        // the config file the editor was given for it is asked about, without reading it
        let status = client.config_info("/src/dprint.json").await.status;
        assert_eq!(status.reason(), Some(NotFormattedReason::ApprovalRequired));
        assert_eq!(status.approvals().len(), 1);
        assert!(is_specifier(&status.approvals()[0]));
        assert_eq!(status.approvals()[0].config_file.uri, uri("/src/dprint.json"));
        let params = PendingApprovalsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/src/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        let pending = client.backend.dprint_pending_approvals(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(pending.approvals.len(), 1);
        assert!(is_specifier(&pending.approvals[0]));
        assert_eq!(pending.status.unwrap().reason(), Some(NotFormattedReason::ApprovalRequired));
        // which includes one in a `.config` directory on another machine
        let status = client.config_info("/src/sub/.config/dprint.json").await.status;
        let specifier = &status.approvals()[0].items[0].specifier;
        assert!(specifier.ends_with(r"dprint.json -> \\server\share\config\dprint.json"), "{}", specifier);
        assert_eq!(status.approvals()[0].config_file.dir_uri, uri("/src/sub"));
        // and its decisions can be forgotten without reading it
        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/src/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        let forgotten = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert!(forgotten.decisions.is_empty());
        assert_eq!(environment.file_read_count("/src/dprint.json"), 0);
        assert_eq!(environment.file_read_count("/src/sub/.config/dprint.json"), 0);

        // it's read once it's allowed (the test environment can't read what's on another machine)
        let approval = client.config_info("/src/dprint.json").await.status.approvals()[0].clone();
        let params = ApproveParams {
          id: approval.id,
          decision: ApprovalDecision::Allow,
          work_done_token: None,
        };
        _ = client.backend.dprint_approve(params, CancellationToken::new()).await;
        assert!(environment.file_read_count("/src/dprint.json") > 0);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_follow_a_path_from_the_editor_to_another_machine_before_it_is_allowed() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/src/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_config_section("shebangs", r##"{ "#!/usr/bin/env test": "txt" }"##);
    })
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin()
        .add_config_section("shebangs", r##"{ "#!/usr/bin/env test": "txt" }"##);
    })
    .build();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.config_info("/src/dprint.json").await.status, FormatStatus::ready());
        // ex. a `git pull` replaced the config file with a symlink to a share, which the
        // editor reports as created because the file was deleted and created
        environment.remove_file("/src/dprint.json").unwrap();
        environment.symlink(network_path("dprint.json"), "/src/dprint.json").unwrap();
        let canonicalize_count = environment.canonicalize_count("/src/dprint.json");
        let read_count = environment.file_read_count("/src/dprint.json");
        client
          .backend
          .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
              uri: uri("/src/dprint.json"),
              typ: FileChangeType::CREATED,
            }],
          })
          .await;
        // the editor is told about it by its path rather than the one on the other machine
        assert_eq!(client.wait_for_config_changes().await, vec![uri("/src/dprint.json")]);
        // and it's asked about without following it
        let config_info = client.config_info("/src/dprint.json").await;
        let approvals = config_info.status.approvals();
        assert_eq!(approvals.len(), 1);
        assert!(approvals[0].config_file.on_another_machine);
        assert_eq!(approvals[0].items[0].kind, ApprovalItemKind::NetworkPath);
        assert_eq!(environment.canonicalize_count("/src/dprint.json"), canonicalize_count);
        assert_eq!(environment.file_read_count("/src/dprint.json"), read_count);

        // nor when the config files that were used are resolved again (ex. after forgetting decisions,
        // or when the approvals file changed), which tells the editor it changed instead
        client
          .backend
          .dprint_forget_decisions(Some(ForgetDecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        assert_eq!(client.take_config_changes(), vec![uri("/src/dprint.json")]);
        assert_eq!(environment.canonicalize_count("/src/dprint.json"), canonicalize_count);
        assert_eq!(environment.file_read_count("/src/dprint.json"), read_count);
        let approval = client.config_info("/src/dprint.json").await.status.approvals()[0].clone();
        assert!(approval.config_file.on_another_machine);

        // the editor is told once reading it is allowed, which is when it may watch it
        client.take_config_changes();
        let _ = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: approval.id,
              decision: ApprovalDecision::Allow,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await;
        assert!(client.take_config_changes().contains(&uri("/src/dprint.json")));

        // the same goes for the file an untitled document is formatted as
        environment.symlink(network_path("Untitled.txt"), "/Untitled.txt").unwrap();
        let mut params = client.format_params("untitled:Untitled-1", "text");
        params.document.language = Some(DocumentLanguage {
          id: "plaintext".to_string(),
          file_extensions: vec![".txt".to_string()],
          file_names: Vec::new(),
        });
        client.format_with(params).await;
        assert_eq!(environment.canonicalize_count("/Untitled.txt"), 0);
        // and its shebang isn't read from the other machine when the editor doesn't provide its text
        environment.symlink(network_path("run"), "/run").unwrap();
        let mut document = document_params("untitled:Untitled-2");
        document.language = Some(DocumentLanguage {
          id: "shellscript".to_string(),
          file_extensions: Vec::new(),
          file_names: vec!["run".to_string()],
        });
        let file_info = client
          .backend
          .dprint_file_info(FileInfoParams { document, text: None }, CancellationToken::new())
          .await
          .unwrap();
        assert_ne!(file_info.status, FormatStatus::ready());
        assert_eq!(environment.canonicalize_count("/run"), 0);
        assert_eq!(environment.file_open_count("/run"), 0);
        client.test_client.take_messages();
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
      // the title fits a configuration file as well as a plugin
      assert_eq!(approval.title, "Download from domains that aren't trusted");
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
fn should_use_an_always_allow_for_a_process_plugin_that_is_not_set_up() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_process_plugin()
        .add_config_section("testProcessPlugin", r#"{ "ending": "first" }"#);
    })
    .build();
  disallow_process_plugins(&environment);
  // ex. another version of dprint, the same one after an upgrade, or another cache directory
  let remove_plugin_cache = || environment.remove_dir_all(environment.get_cache_dir().join("plugins")).unwrap();
  let approvals_file_path = crate::configuration::get_editor_approvals_file_path(&environment).unwrap();
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(client.approve(&approval.id, ApprovalDecision::AlwaysAllow).await, Vec::new());
    }
    .boxed_local()
  });
  // it's stored with the key of its configuration
  let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
  assert_eq!(decisions.len(), 1);
  assert_eq!(decisions[0].approval.config_key.as_deref(), Some("testProcessPlugin"));

  remove_plugin_cache();
  run_session(&environment, |client| {
    async move {
      // it's set up without asking because its configuration was allowed
      let info = client.config_info("/dprint.json").await;
      assert_eq!(info.status, FormatStatus::ready());
      assert_eq!(info.plugins.unwrap()[0].config_key, "testProcessPlugin");
      assert_eq!(client.format("/file.txt_ps", "text").await, formatted("text", "text_first"));
    }
    .boxed_local()
  });

  // an allow that was stored without the key of its configuration (ex. by another version of dprint)
  // is used with the configuration of any key
  let text = environment.read_file(&approvals_file_path).unwrap();
  let mut approvals = serde_json::from_str::<serde_json::Value>(&text).unwrap();
  approvals["processPluginDecisions"][0].as_object_mut().unwrap().remove("configKey");
  environment.write_file(&approvals_file_path, &approvals.to_string()).unwrap();
  remove_plugin_cache();
  run_session(&environment, |client| {
    async move {
      assert_eq!(client.config_info("/dprint.json").await.status, FormatStatus::ready());
      assert_eq!(client.format("/file.txt_ps", "text").await, formatted("text", "text_first"));
    }
    .boxed_local()
  });

  // IMPORTANT: it's checked with its own configuration once it's set up, before it runs with it
  let config_text = environment.read_file("/dprint.json").unwrap();
  let mut config = serde_json::from_str::<serde_json::Value>(&config_text).unwrap();
  // ex. the configuration of another plugin is what the process plugin had
  config["otherPlugin"] = json!({ "ending": "first" });
  config["testProcessPlugin"] = json!({ "ending": "second" });
  environment.write_file("/dprint.json", &config.to_string()).unwrap();
  remove_plugin_cache();
  run_session(&environment, |client| {
    async move {
      let status = client.config_info("/dprint.json").await.status;
      assert_eq!(status.reason(), Some(NotFormattedReason::ApprovalRequired));
      let approval = status.approvals()[0].clone();
      assert_eq!(approval.items[0].configuration, Some(json!({ "ending": "second" })));
      assert!(approval.message.contains("changed since they were allowed."), "{}", approval.message);
    }
    .boxed_local()
  });

  // and a configuration that wasn't allowed isn't set up
  environment.write_file("/dprint.json", &config_text.replace("first", "second")).unwrap();
  remove_plugin_cache();
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(approval.items[0].plugin, None);
    }
    .boxed_local()
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
      .contains(&format!("(`{}` from `https://raw.githubusercontent.com/other/registry/main`)", PLUGIN)),
    "{}",
    approval.message
  );
  // what was shown is part of the id
  let other_registry = get_approval(Some("@dprint:registry=https://raw.githubusercontent.com/another/registry/main/"));
  assert_ne!(other_registry.id, approval.id);
  // the default registry isn't shown
  let default_registry = get_approval(None);
  assert_eq!(default_registry.items[0].registry, None);
  assert!(default_registry.message.contains(&format!("(`{}`)", PLUGIN)), "{}", default_registry.message);
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
            config_hash: Some("1:hash".to_string()),
            config_key: None,
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
            config_key: None,
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
fn should_say_when_always_allowing_a_process_plugin_that_is_not_set_up_could_not_be_stored() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build(); // not initialized, so the process plugin is only set up when it's allowed
  disallow_process_plugins(&environment);
  // writing the approvals file fails when it's a directory
  environment
    .mk_dir_all(crate::configuration::get_editor_approvals_file_path(&environment).unwrap())
    .unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
        // it's stored once it's set up, which approving it does
        let result = client
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
          .unwrap();
        assert_eq!(result.config_approvals.status, FormatStatus::ready());
        let notice = result.notice.unwrap();
        assert!(
          notice.message.starts_with("Failed storing the decision for the process plugins: "),
          "{}",
          notice.message
        );
        // it's used for the session
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));

        // a decision for process plugins that are set up fails to be stored right away
        let text = environment.read_file("/dprint.json").unwrap();
        environment
          .write_file("/dprint.json", &text.replacen('{', r#"{ "testProcessPlugin": { "ending": "changed" }, "#, 1))
          .unwrap();
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        let result = client
          .backend
          .dprint_approve(
            ApproveParams {
              id: approval.id.clone(),
              decision: ApprovalDecision::AlwaysDeny,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
          .await
          .unwrap();
        assert!(result.notice.is_some());
        assert!(result.config_approvals.approvals[0].denied);
        environment.take_stderr_messages();
      }
      .boxed_local()
    }
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
fn should_say_when_always_allowing_a_process_plugin_could_not_be_stored_once_what_it_needed_downloaded_is_allowed() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL).add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  // writing the approvals file fails when it's a directory
  environment
    .mk_dir_all(crate::configuration::get_editor_approvals_file_path(&environment).unwrap())
    .unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approve = |id: String, decision: ApprovalDecision| {
          client.backend.dprint_approve(
            ApproveParams {
              id,
              decision,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
        };
        let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
        // the process plugins can't be set up until the download is allowed, so nothing failed yet
        let result = approve(approvals[1].id.clone(), ApprovalDecision::AlwaysAllow).await.unwrap();
        assert!(result.notice.is_none());
        // allowing the download sets them up, which is what says it
        let result = approve(approvals[0].id.clone(), ApprovalDecision::Allow).await.unwrap();
        assert_eq!(result.config_approvals.status, FormatStatus::ready());
        let notice = result.notice.unwrap();
        assert!(
          notice.message.starts_with("Failed storing the decision for the process plugins: "),
          "{}",
          notice.message
        );
        environment.take_stderr_messages();
      }
      .boxed_local()
    }
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
fn should_only_use_always_decisions_for_the_session_with_the_approvals_of_a_newer_version_of_dprint() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build(); // not initialized, so the process plugin is only set up when it's allowed
  disallow_process_plugins(&environment);
  // a newer version of dprint wrote it
  let file_path = crate::configuration::get_editor_approvals_file_path(&environment).unwrap();
  let text = r#"{ "version": 999, "minVersion": 999, "processPluginDecisions": [] }"#;
  environment.write_file(&file_path, text).unwrap();
  let expected_message = format!(
    "This version of dprint is older than the one that stored the approval decisions in {}, so this decision is only kept until the editor restarts. Update dprint to remember it.",
    file_path.display()
  );
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approve = |id: String, decision: ApprovalDecision| {
          client.backend.dprint_approve(
            ApproveParams {
              id,
              decision,
              work_done_token: None,
            },
            CancellationToken::new(),
          )
        };
        // it's stored once it's set up, which approving it does
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        let result = approve(approval.id.clone(), ApprovalDecision::AlwaysAllow).await.unwrap();
        assert_eq!(result.config_approvals.status, FormatStatus::ready());
        assert_eq!(result.notice.unwrap().message, expected_message);
        // it's used for the session
        assert_eq!(client.format("/file.txt", "text").await, formatted("text", "text_formatted"));

        // a decision for process plugins that are set up isn't stored right away
        let config_text = environment.read_file("/dprint.json").unwrap();
        environment
          .write_file(
            "/dprint.json",
            &config_text.replacen('{', r#"{ "testProcessPlugin": { "ending": "changed" }, "#, 1),
          )
          .unwrap();
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        let result = approve(approval.id.clone(), ApprovalDecision::AlwaysDeny).await.unwrap();
        assert_eq!(result.notice.unwrap().message, expected_message);
        assert!(result.config_approvals.approvals[0].denied);
        // and it's not an "Always" decision
        assert_eq!(result.config_approvals.approvals[0].previous_decision, Some(ApprovalDecision::Deny));

        // the file isn't written
        assert_eq!(environment.read_file(&file_path).unwrap(), text);
        environment.take_stderr_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_only_forget_the_decisions_of_the_session_with_the_approvals_of_a_newer_version_of_dprint() {
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
  let text = approvals.to_string();
  environment.write_file(&file_path, &text).unwrap();
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        // the user allows it for the session
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert!(approval.denied);
        assert!(client.approve(&approval.id, ApprovalDecision::Allow).await.is_empty());
        let params = ForgetDecisionsParams {
          config_file: Some(ConfigFileIdentifier {
            uri: uri("/a/dprint.json"),
            dir_uri: None,
          }),
          work_done_token: None,
        };
        let result = client.backend.dprint_forget_decisions(Some(params), CancellationToken::new()).await.unwrap();
        assert_eq!(
          result.notice.unwrap().message,
          format!(
            "This version of dprint is older than the one that stored the approval decisions in {}, so the ones stored there couldn't be forgotten. Update dprint to forget them.",
            file_path.display()
          )
        );
        // the session's decision is forgotten, so the stored denial is used again
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert!(approval.denied);
        assert_eq!(approval.previous_decision, Some(ApprovalDecision::AlwaysDeny));
        // and the file isn't written
        assert_eq!(environment.read_file(&file_path).unwrap(), text);
        environment.take_stderr_messages();
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

#[test]
fn should_keep_the_decisions_of_the_session_when_the_server_is_started_again_for_it() {
  const ALLOWED_URL: &str = "https://example.com/allowed/test-plugin.wasm";
  const DENIED_URL: &str = "https://example.com/denied/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_wasm_plugin_at_url(ALLOWED_URL)
    .add_remote_wasm_plugin_at_url(DENIED_URL)
    .add_remote_process_plugin()
    .with_local_config("/allowed-download/dprint.json", |c| {
      c.add_plugin(ALLOWED_URL);
    })
    .with_local_config("/denied-download/dprint.json", |c| {
      c.add_plugin(DENIED_URL);
    })
    .with_local_config("/allowed/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .with_local_config("/denied/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build(); // not initialized, so the plugins aren't set up
  disallow_process_plugins(&environment);
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      for (config_file, decision) in [
        ("/allowed-download/dprint.json", ApprovalDecision::Allow),
        ("/denied-download/dprint.json", ApprovalDecision::Deny),
        ("/allowed/dprint.json", ApprovalDecision::Allow),
        ("/denied/dprint.json", ApprovalDecision::Deny),
      ] {
        let approval = client.config_info(config_file).await.status.approvals()[0].clone();
        client.approve(&approval.id, decision).await;
      }
    }
    .boxed_local()
  });
  assert!(environment.path_exists(get_session_file_path(&environment, SESSION_ID)));

  // a server started again for the session uses them like the server that exited
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert_eq!(client.config_info("/allowed/dprint.json").await.status, FormatStatus::ready());
      assert!(client.config_info("/denied/dprint.json").await.status.approvals()[0].denied);
      assert!(client.config_info("/denied-download/dprint.json").await.status.approvals()[0].denied);
      assert_eq!(client.config_info("/allowed-download/dprint.json").await.status, FormatStatus::ready());
      // the allowed download is still allowed (the plugin is in the cache too)
      let decisions = client.decisions().await;
      assert!(
        decisions
          .iter()
          .any(|decision| decision.allows && decision.config_file.is_none() && decision.specifiers == vec![ALLOWED_URL]),
        "{:?}",
        decisions
      );
    }
    .boxed_local()
  });

  // and one of another session doesn't (ex. after the restart command)
  run_session_with(&environment, session_params(OTHER_SESSION_ID), |client| {
    async move {
      let approval = client.config_info("/allowed/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
      assert!(!approval.denied);
      assert!(!client.config_info("/denied/dprint.json").await.status.approvals()[0].denied);
      assert!(!client.config_info("/denied-download/dprint.json").await.status.approvals()[0].denied);
      assert!(!client.decisions().await.iter().any(|decision| decision.allows));
    }
    .boxed_local()
  });
}

#[test]
fn should_keep_allowing_a_process_plugin_to_be_set_up_when_the_server_is_started_again_for_the_session() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL).add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
      // the process plugins can't be set up until the download is allowed
      let remaining = client.approve(&approvals[1].id, ApprovalDecision::Allow).await;
      assert_eq!(
        remaining.iter().map(|approval| approval.kind).collect::<Vec<_>>(),
        vec![ApprovalKind::Downloads]
      );
    }
    .boxed_local()
  });
  run_session_with(&environment, session_params(SESSION_ID), |client| {
    async move {
      let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
      assert_eq!(
        approvals.iter().map(|approval| approval.kind).collect::<Vec<_>>(),
        vec![ApprovalKind::Downloads]
      );
      // so allowing the download sets them up
      assert_eq!(client.approve(&approvals[0].id, ApprovalDecision::Allow).await, Vec::new());
      assert_eq!(client.format("/file.txt_ps", "text").await, formatted("text", "text_formatted_process"));
    }
    .boxed_local()
  });
}

#[test]
fn should_delete_the_session_file_when_the_client_stops_the_server() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  run_session_with(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        client.approve(&approval.id, ApprovalDecision::Deny).await;
        // it's kept before the decision is responded to
        let session_file_path = get_session_file_path(&environment, SESSION_ID);
        let text = environment.read_file(&session_file_path).unwrap();
        assert!(text.contains(UNTRUSTED_PLUGIN_URL), "{}", text);
        // the session only ends on the exit after the shutdown, so a server that's killed
        // after shutting down (ex. one that stopped responding) keeps it
        client.backend.shutdown().await.unwrap();
        assert_eq!(environment.read_file(&session_file_path).unwrap(), text);
      }
      .boxed_local()
    }
  });
  // the session ended once the client exited the server
  assert!(!environment.path_exists(get_session_file_path(&environment, SESSION_ID)));
  run_session_with(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert!(!client.config_info("/dprint.json").await.status.approvals()[0].denied);
    }
    .boxed_local()
  });
}

#[test]
fn should_delete_the_session_files_that_were_not_written_for_a_while() {
  let environment = TestEnvironmentBuilder::new().build();
  environment.set_fs_time(1_000_000);
  let stale_file_path = get_session_file_path(&environment, OTHER_SESSION_ID);
  environment.mk_dir_all(stale_file_path.parent().unwrap()).unwrap();
  environment.write_file(&stale_file_path, "{}").unwrap();
  // it's kept for a week
  environment.set_fs_time(1_000_000 + 6 * 24 * 60 * 60);
  run_session_with(&environment, session_params(SESSION_ID), |_| async {}.boxed_local());
  assert!(environment.path_exists(&stale_file_path));
  environment.set_fs_time(1_000_000 + 8 * 24 * 60 * 60);
  // which is checked by a server of any session
  run_session(&environment, |_| async {}.boxed_local());
  assert!(!environment.path_exists(&stale_file_path));
}

#[test]
fn should_ignore_a_session_id_that_is_not_valid() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  for session_id in [json!("too-short"), json!("../../0123456789abcdef"), json!(1234567890123456789u64)] {
    let params = || {
      let mut params = session_params(SESSION_ID);
      params.capabilities.experimental.as_mut().unwrap()["dprint"]["sessionId"] = session_id.clone();
      params
    };
    run_session_until_the_server_exits(&environment, params(), |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert!(client.approve(&approval.id, ApprovalDecision::Deny).await[0].denied);
      }
      .boxed_local()
    });
    // so the decisions are lost like without one
    let sessions_dir = get_session_file_path(&environment, SESSION_ID).parent().unwrap().to_path_buf();
    assert!(environment.dir_info(&sessions_dir).map(|entries| entries.is_empty()).unwrap_or(true));
    run_session_until_the_server_exits(&environment, params(), |client| {
      async move {
        assert!(!client.config_info("/dprint.json").await.status.approvals()[0].denied);
      }
      .boxed_local()
    });
  }
}

#[test]
fn should_forget_the_decisions_of_the_session_in_its_session_file() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/allowed/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .with_local_config("/denied/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        for (config_file, decision) in [
          ("/allowed/dprint.json", ApprovalDecision::Allow),
          ("/denied/dprint.json", ApprovalDecision::Deny),
        ] {
          let approval = client.config_info(config_file).await.status.approvals()[0].clone();
          client.approve(&approval.id, decision).await;
        }
        client
          .backend
          .dprint_forget_decisions(Some(ForgetDecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        let text = environment.read_file(get_session_file_path(&environment, SESSION_ID)).unwrap();
        let state = serde_json::from_str::<serde_json::Value>(&text).unwrap();
        assert_eq!(state["processPluginDecisions"], json!([]), "{}", text);
        assert_eq!(state["processPluginSetUpGrants"], json!([]), "{}", text);
      }
      .boxed_local()
    }
  });
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert!(!client.config_info("/allowed/dprint.json").await.status.approvals().is_empty());
      assert!(!client.config_info("/denied/dprint.json").await.status.approvals()[0].denied);
    }
    .boxed_local()
  });
}

#[test]
fn should_not_keep_what_was_forgotten_in_a_session_file_that_failed_to_be_written() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        client.approve(&approval.id, ApprovalDecision::Allow).await;
        let session_file_path = get_session_file_path(&environment, SESSION_ID);
        assert!(environment.path_exists(&session_file_path));
        // ex. the disk is full and another process has the file open
        environment.set_file_write_fails(&session_file_path, true);
        environment.set_file_remove_fails(&session_file_path, true);
        let forget = || async {
          client
            .backend
            .dprint_forget_decisions(Some(ForgetDecisionsParams::default()), CancellationToken::new())
            .await
            .unwrap()
        };
        // so the allow may come back, which the user is told about
        let notice = forget().await.notice.unwrap();
        assert!(notice.message.contains("may come back if dprint restarts"), "{}", notice.message);
        assert!(environment.path_exists(&session_file_path));
        // otherwise the file is deleted
        environment.set_file_remove_fails(&session_file_path, false);
        assert_eq!(forget().await.notice, None);
        assert!(!environment.path_exists(&session_file_path));
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
  // so a server started again for the session asks again
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      let approvals = client.config_info("/dprint.json").await.status.approvals().to_vec();
      assert_eq!(approvals[0].kind, ApprovalKind::ProcessPlugins);
    }
    .boxed_local()
  });
}

#[test]
fn should_take_over_the_session_file_of_a_session_id_another_server_used() {
  const FIRST_URL: &str = "https://example.com/first/test-plugin.wasm";
  const SECOND_URL: &str = "https://example.com/second/test-plugin.wasm";
  const OTHER_URL: &str = "https://example.com/other/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(FIRST_URL)
    .add_remote_wasm_plugin_at_url(SECOND_URL)
    .with_local_config("/first/dprint.json", |c| {
      c.add_plugin(FIRST_URL);
    })
    .with_local_config("/second/dprint.json", |c| {
      c.add_plugin(SECOND_URL);
    })
    .build();
  // ex. the previous server of the session hasn't exited yet, which allowed a download
  let session_file_path = get_session_file_path(&environment, SESSION_ID);
  environment.mk_dir_all(session_file_path.parent().unwrap()).unwrap();
  let other_server_lock = environment.try_lock_file(session_file_path.with_extension("lock")).unwrap().unwrap();
  let other_text = format!(
    r#"{{ "version": 1, "allowedDownloads": [{{ "download": "{}" }}], "deniedDownloads": [] }}"#,
    OTHER_URL
  );
  environment.write_file(&session_file_path, &other_text).unwrap();
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        let messages = client.test_client.take_messages();
        assert!(
          messages
            .iter()
            .any(|(_, message)| message.contains("Not keeping what's decided for the session") && message.contains("which it keeps trying")),
          "{:?}",
          messages
        );
        // what the other server allowed isn't used
        assert!(!client.decisions().await.iter().any(|decision| decision.specifiers == vec![OTHER_URL]));
        // and forgetting decisions says they aren't kept either
        let result = client
          .backend
          .dprint_forget_decisions(Some(ForgetDecisionsParams::default()), CancellationToken::new())
          .await
          .unwrap();
        let notice = result.notice.unwrap();
        assert!(notice.message.contains("won't be kept if dprint restarts"), "{}", notice.message);
        assert_eq!(environment.read_file(&session_file_path).unwrap(), other_text);
        let deny = |config_file: &'static str| {
          let client = &client;
          async move {
            let approval = client.config_info(config_file).await.status.approvals()[0].clone();
            client
              .backend
              .dprint_approve(
                ApproveParams {
                  id: approval.id,
                  decision: ApprovalDecision::Deny,
                  work_done_token: None,
                },
                CancellationToken::new(),
              )
              .await
              .unwrap()
          }
        };
        // the user is told the decision isn't kept, and the other server's file isn't changed
        let notice = deny("/first/dprint.json").await.notice.unwrap();
        assert!(notice.message.contains("won't be kept if dprint restarts"), "{}", notice.message);
        assert_eq!(environment.read_file(&session_file_path).unwrap(), other_text);

        // ex. the other server exited without deleting its file
        drop(other_server_lock);
        assert_eq!(deny("/second/dprint.json").await.notice, None);
        // so this server's decisions replace the other server's
        let text = environment.read_file(&session_file_path).unwrap();
        assert!(text.contains(FIRST_URL) && text.contains(SECOND_URL), "{}", text);
        assert!(!text.contains(OTHER_URL), "{}", text);
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
  // and a server started again for the session uses them
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert!(client.config_info("/first/dprint.json").await.status.approvals()[0].denied);
      assert!(client.config_info("/second/dprint.json").await.status.approvals()[0].denied);
    }
    .boxed_local()
  });
}

#[test]
fn should_take_over_the_session_file_without_a_request_once_the_other_server_exited() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  const OTHER_URL: &str = "https://example.com/other/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  let session_file_path = get_session_file_path(&environment, SESSION_ID);
  environment.mk_dir_all(session_file_path.parent().unwrap()).unwrap();
  let other_server_lock = environment.try_lock_file(session_file_path.with_extension("lock")).unwrap().unwrap();
  let other_text = format!(
    r#"{{ "version": 1, "allowedDownloads": [{{ "download": "{}" }}], "deniedDownloads": [] }}"#,
    OTHER_URL
  );
  environment.write_file(&session_file_path, &other_text).unwrap();
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert!(client.approve(&approval.id, ApprovalDecision::Deny).await[0].denied);
        assert_eq!(environment.read_file(&session_file_path).unwrap(), other_text);
        // ex. the other server was killed, which doesn't delete its file
        drop(other_server_lock);
        // the server takes the file over without anything being decided, so the other server's
        // decisions aren't used if this server stops
        let start = std::time::Instant::now();
        loop {
          let text = environment.read_file(&session_file_path).unwrap();
          if !text.contains(OTHER_URL) {
            assert!(text.contains(UNTRUSTED_PLUGIN_URL), "{}", text);
            break;
          }
          assert!(start.elapsed() < std::time::Duration::from_secs(30), "the session file wasn't taken over");
          tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert!(client.config_info("/dprint.json").await.status.approvals()[0].denied);
    }
    .boxed_local()
  });
}

#[test]
fn should_keep_the_allows_of_the_session_with_an_approvals_file_when_the_server_is_started_again() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/always/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .with_local_config("/session/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .build();
  disallow_process_plugins(&environment);
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      for (config_file, decision) in [
        ("/always/dprint.json", ApprovalDecision::AlwaysAllow),
        ("/session/dprint.json", ApprovalDecision::Allow),
      ] {
        let approval = client.config_info(config_file).await.status.approvals()[0].clone();
        assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
        client.approve(&approval.id, decision).await;
      }
    }
    .boxed_local()
  });
  // the session file has the fingerprint of the approvals file the server stored the "Always Allow" in
  let text = environment.read_file(get_session_file_path(&environment, SESSION_ID)).unwrap();
  let fingerprint = serde_json::from_str::<serde_json::Value>(&text).unwrap()["approvalsFileFingerprint"].clone();
  assert!(fingerprint.as_str().is_some_and(|fingerprint| fingerprint != "none"), "{}", text);

  // which didn't change since, so the allow for the session is used again
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert_eq!(client.config_info("/session/dprint.json").await.status, FormatStatus::ready());
      assert_eq!(client.config_info("/always/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_restore_a_denial_of_downloads_with_items_that_are_not_known() {
  const UNTRUSTED_PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(UNTRUSTED_PLUGIN_URL)
    .with_local_config("/dprint.json", |c| {
      c.add_plugin(UNTRUSTED_PLUGIN_URL);
    })
    .build();
  // ex. kept by a newer version of dprint with a kind of item this version doesn't know, and without
  // the id because it's made again from the downloads
  let session_file_path = get_session_file_path(&environment, SESSION_ID);
  environment.mk_dir_all(session_file_path.parent().unwrap()).unwrap();
  let text = format!(
    r#"{{
      "version": 1,
      "allowedDownloads": [{{ "download": "npm-registry:https://npm.example.com/", "item": {{ "specifier": "x", "kind": "newKind" }} }}],
      "deniedDownloads": [{{
        "configKey": "file:///dprint.json",
        "downloads": ["{url}"],
        "configFile": {{ "uri": "file:///dprint.json", "dirUri": "file:///" }},
        "items": [{{ "specifier": "{url}", "kind": "newKind" }}]
      }}]
    }}"#,
    url = UNTRUSTED_PLUGIN_URL
  );
  environment.write_file(&session_file_path, &text).unwrap();
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert!(client.config_info("/dprint.json").await.status.approvals()[0].denied);
      // the decision shows what was denied without the items it doesn't know
      let decisions = client.decisions().await;
      let denial = decisions.iter().find(|decision| !decision.allows).unwrap();
      assert_eq!(denial.specifiers, vec![UNTRUSTED_PLUGIN_URL.to_string()]);
      // a url may be a plugin or a config file, so its item isn't known
      assert_eq!(denial.items, None);
      // while what a registry is can be told from what was allowed
      let allow = decisions.iter().find(|decision| decision.allows).unwrap();
      assert_eq!(allow.specifiers, vec!["https://npm.example.com/".to_string()]);
      let item = &allow.items.as_ref().unwrap()[0];
      assert_eq!(item.registry.as_deref(), Some("https://npm.example.com/"));
    }
    .boxed_local()
  });
}

#[test]
fn should_keep_denying_downloads_that_were_denied_together() {
  const FIRST_URL: &str = "https://example.com/first/test-plugin.wasm";
  const SECOND_URL: &str = "https://example.com/second/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(FIRST_URL)
    .add_remote_wasm_plugin_at_url(SECOND_URL)
    .with_local_config("/both/dprint.json", |c| {
      c.add_plugin(FIRST_URL).add_plugin(SECOND_URL);
    })
    .with_local_config("/first/dprint.json", |c| {
      c.add_plugin(FIRST_URL);
    })
    .build();
  run_session(&environment, |client| {
    async move {
      let approval = client.config_info("/both/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(specifiers(&approval), vec![FIRST_URL, SECOND_URL]);
      client.approve(&approval.id, ApprovalDecision::Deny).await;
      // one of them is allowed for another config file
      let approval = client.config_info("/first/dprint.json").await.status.approvals()[0].clone();
      client.approve(&approval.id, ApprovalDecision::Allow).await;
      // so the other is what's needed now, which is still denied
      let approval = client.config_info("/both/dprint.json").await.status.approvals()[0].clone();
      assert_eq!(specifiers(&approval), vec![SECOND_URL]);
      assert!(approval.denied);
      assert_eq!(approval.previous_decision, Some(ApprovalDecision::Deny));

      // and allowing it (ex. with the approve command) replaces the denial
      client.approve(&approval.id, ApprovalDecision::Allow).await;
      let decisions = client.decisions().await;
      assert!(!decisions.iter().any(|decision| !decision.allows), "{:?}", decisions);
    }
    .boxed_local()
  });
}

#[test]
fn should_forget_an_always_allow_that_is_still_being_stored() {
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .add_remote_process_plugin()
    .with_local_config("/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_remote_process_plugin();
    })
    .initialize()
    .build();
  disallow_process_plugins(&environment);
  run_session(&environment, {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(approval.kind, ApprovalKind::ProcessPlugins);
        // ex. `dprint fmt` is storing its decisions
        let lock_path = crate::configuration::get_editor_approvals_file_path(&environment)
          .unwrap()
          .with_file_name(".editor-approvals.lock");
        let other_lock = environment.try_lock_file(&lock_path).unwrap().unwrap();
        let approve = client.backend.dprint_approve(
          ApproveParams {
            id: approval.id.clone(),
            decision: ApprovalDecision::AlwaysAllow,
            work_done_token: None,
          },
          CancellationToken::new(),
        );
        // the user forgets the decisions while it waits for the lock
        let forget = async {
          tokio::time::sleep(std::time::Duration::from_millis(200)).await;
          client
            .backend
            .dprint_forget_decisions(Some(ForgetDecisionsParams::default()), CancellationToken::new())
            .await
            .unwrap()
        };
        let release = async {
          tokio::time::sleep(std::time::Duration::from_millis(400)).await;
          drop(other_lock);
        };
        let (approve_result, _, ()) = dprint_core::async_runtime::future::join3(approve, forget, release).await;
        approve_result.unwrap();
        // forgetting waited for it to be stored, so it's forgotten too
        let decisions = crate::configuration::read_editor_approvals(&environment).unwrap().process_plugin_decisions;
        assert!(decisions.iter().all(|decision| !decision.allowed), "{:?}", decisions);
        assert_eq!(
          client.config_info("/dprint.json").await.status.approvals()[0].kind,
          ApprovalKind::ProcessPlugins
        );
        client.test_client.take_messages();
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_allow_the_packages_of_an_npm_registry_with_a_path() {
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use crate::test_helpers::create_test_npm_tarball;

  const PACKUMENT_URL: &str = "https://corp.example.com/npm/repo/test-plugin";
  const TARBALL_URL: &str = "https://corp.example.com/npm/repo/test-plugin/-/test-plugin-1.0.0.tgz";
  let environment = TestEnvironmentBuilder::new()
    .with_local_config("/project/dprint.json", |c| {
      c.add_plugin("npm:test-plugin@1.0.0");
    })
    .write_file("/project/.npmrc", "registry=https://corp.example.com/npm/repo/")
    .build();
  let packument = json!({ "versions": { "1.0.0": { "dist": { "tarball": TARBALL_URL } } } });
  environment.add_remote_file_bytes(PACKUMENT_URL, packument.to_string().into_bytes());
  environment.add_remote_file_bytes(TARBALL_URL, create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]));
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      let approval = client.config_info("/project/dprint.json").await.status.approvals()[0].clone();
      // it's shown as the registry, which allows every url under it unlike another url
      assert_eq!(specifiers(&approval), vec!["https://corp.example.com/npm/repo/"]);
      assert_eq!(approval.items[0].registry.as_deref(), Some("https://corp.example.com/npm/repo/"));
      assert!(
        approval.message.contains("is an npm registry, so allowing it allows every url under it"),
        "{}",
        approval.message
      );
      // so its packages are downloaded from under its path
      assert_eq!(client.approve(&approval.id, ApprovalDecision::Allow).await, Vec::new());
      assert_eq!(client.config_info("/project/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
  // and a server started again for the session still allows it
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), |client| {
    async move {
      assert_eq!(client.config_info("/project/dprint.json").await.status, FormatStatus::ready());
    }
    .boxed_local()
  });
}

#[test]
fn should_keep_denying_a_download_under_an_npm_registry_that_is_allowed() {
  use crate::test_helpers::WASM_PLUGIN_BYTES;
  use crate::test_helpers::create_test_npm_tarball;

  const EXTENDS_URL: &str = "https://npm.example.com/a.json";
  const PACKUMENT_URL: &str = "https://npm.example.com/test-plugin";
  const TARBALL_URL: &str = "https://npm.example.com/test-plugin/-/test-plugin-1.0.0.tgz";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin()
    .with_local_config("/a/dprint.json", |c| {
      c.add_remote_wasm_plugin().add_config_section("extends", &format!(r#""{}""#, EXTENDS_URL));
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_plugin("npm:test-plugin@1.0.0");
    })
    .write_file("/b/.npmrc", "registry=https://npm.example.com/")
    .build();
  environment.add_remote_file_bytes(EXTENDS_URL, b"{}".to_vec());
  let packument = json!({ "versions": { "1.0.0": { "dist": { "tarball": TARBALL_URL } } } });
  environment.add_remote_file_bytes(PACKUMENT_URL, packument.to_string().into_bytes());
  environment.add_remote_file_bytes(TARBALL_URL, create_test_npm_tarball(&[("package/plugin.wasm", WASM_PLUGIN_BYTES)]));
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(specifiers(&approval), vec![EXTENDS_URL]);
        client.approve(&approval.id, ApprovalDecision::Deny).await;
        // the user allows the registry for another config file, which allows every url under it
        let approval = client.config_info("/b/dprint.json").await.status.approvals()[0].clone();
        assert_eq!(specifiers(&approval), vec!["https://npm.example.com/"]);
        client.approve(&approval.id, ApprovalDecision::Allow).await;
        assert_eq!(client.config_info("/b/dprint.json").await.status, FormatStatus::ready());
        // but not what the user denied
        let approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
        assert!(approval.denied);
        assert_eq!(environment.remote_file_request_count(EXTENDS_URL), 0);
      }
      .boxed_local()
    }
  });
  // which is kept for a server started again for the session
  run_session_until_the_server_exits(&environment, session_params(SESSION_ID), {
    let environment = environment.clone();
    move |client| {
      async move {
        assert_eq!(client.config_info("/b/dprint.json").await.status, FormatStatus::ready());
        assert!(client.config_info("/a/dprint.json").await.status.approvals()[0].denied);
        assert_eq!(environment.remote_file_request_count(EXTENDS_URL), 0);
        // until it's forgotten, when the registry allows it
        client
          .backend
          .dprint_forget_decisions(
            Some(ForgetDecisionsParams {
              config_file: Some(ConfigFileIdentifier {
                uri: uri("/a/dprint.json"),
                dir_uri: None,
              }),
              work_done_token: None,
            }),
            CancellationToken::new(),
          )
          .await
          .unwrap();
        assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
        assert_eq!(environment.remote_file_request_count(EXTENDS_URL), 1);
      }
      .boxed_local()
    }
  });
}

#[test]
fn should_not_deny_a_download_that_was_allowed_exactly_since() {
  const PLUGIN_URL: &str = "https://example.com/test-plugin.wasm";
  let environment = TestEnvironmentBuilder::new()
    .add_remote_wasm_plugin_at_url(PLUGIN_URL)
    .with_local_config("/a/dprint.json", |c| {
      c.add_plugin(PLUGIN_URL);
    })
    .with_local_config("/b/dprint.json", |c| {
      c.add_plugin(PLUGIN_URL);
    })
    .build();
  run_session(&environment, |client| {
    async move {
      let a_approval = client.config_info("/a/dprint.json").await.status.approvals()[0].clone();
      let b_approval = client.config_info("/b/dprint.json").await.status.approvals()[0].clone();
      // the user allows it for one config file while the prompt of the other is still shown
      client.approve(&b_approval.id, ApprovalDecision::Allow).await;
      let result = client
        .backend
        .dprint_approve(
          ApproveParams {
            id: a_approval.id,
            decision: ApprovalDecision::Deny,
            work_done_token: None,
          },
          CancellationToken::new(),
        )
        .await
        .unwrap();
      // which doesn't do anything, because it's allowed exactly, so the user is told and it isn't kept
      let notice = result.notice.unwrap();
      assert!(notice.message.starts_with("What you denied was allowed since"), "{}", notice.message);
      assert_eq!(result.config_approvals.approvals, Vec::new());
      assert_eq!(result.config_approvals.status, FormatStatus::ready());
      assert!(!client.decisions().await.iter().any(|decision| !decision.allows));
      assert_eq!(client.config_info("/a/dprint.json").await.status, FormatStatus::ready());
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

  async fn decisions(&self) -> Vec<Decision> {
    self
      .backend
      .dprint_decisions(Some(DecisionsParams::default()), CancellationToken::new())
      .await
      .unwrap()
      .decisions
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

/// Runs a session of an editor extension whose server exits without the
/// client stopping it (ex. it crashed), so the session doesn't end.
fn run_session_until_the_server_exits(
  environment: &TestEnvironment,
  params: InitializeParams,
  action: impl FnOnce(Client) -> LocalBoxFuture<'static, ()> + 'static,
) {
  let environment = environment.clone();
  environment.clone().run_in_runtime(async move {
    let plugin_cache = PluginCache::new(environment.clone());
    let plugin_resolver = Rc::new(PluginResolver::new(environment.clone(), plugin_cache, Default::default()));
    let (tx, rx) = mpsc::unbounded_channel();
    let recv_task = start_message_handler(&environment, &plugin_resolver, None, RemoteCacheMode::Use, Default::default(), rx);
    let test_client = Arc::new(TestClient::default());
    let backend = Rc::new(Backend::new(ClientWrapper::new(test_client.clone()), environment.clone(), tx, false));
    initialize_backend(&backend, params).await;
    action(Client {
      backend: backend.clone(),
      test_client: test_client.clone(),
    })
    .await;
    drop(backend);
    // it stopped without a shutdown request
    assert!(!recv_task.await.unwrap());
    plugin_resolver.clear_and_shutdown_initialized().await;
    test_client.take_messages();
  });
}

const SESSION_ID: &str = "0f8c2b1e-5a4d-4c3b-9e2f-7a6b5c4d3e2f";
const OTHER_SESSION_ID: &str = "9d8c7b6a-5f4e-4d3c-8b2a-1f0e9d8c7b6a";

/// Gets the params of an editor extension that provides a session id.
fn session_params(session_id: &str) -> InitializeParams {
  let mut params = editor_extension_params();
  params.capabilities.experimental = Some(json!({ "dprint": { "versions": [1], "sessionId": session_id } }));
  params
}

fn get_session_file_path(environment: &TestEnvironment, session_id: &str) -> PathBuf {
  environment.get_cache_dir().join("editor-sessions").join(format!("{}.json", session_id))
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
  fn claim(&self) -> bool;
}

impl FormatResultExt for FormatResult {
  fn not_formatted_reason(&self) -> Option<NotFormattedReason> {
    match self {
      FormatResult::NotFormatted(not_formatted) => Some(not_formatted.reason),
      _ => None,
    }
  }

  fn claim(&self) -> bool {
    match self {
      FormatResult::NotFormatted(not_formatted) => not_formatted.claim,
      FormatResult::Failed { claim, .. } => *claim,
      FormatResult::Formatted { .. } => true,
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
    on_another_machine: false,
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

/// Gets a path on another machine, which is a UNC path on Windows and a path
/// in `/net` on macOS and Linux.
fn network_path(path: &str) -> String {
  if cfg!(windows) {
    format!(r"\\server\share\{}", path.replace('/', "\\"))
  } else {
    format!("/net/server/share/{}", path)
  }
}
