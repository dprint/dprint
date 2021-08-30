import ow from "ow";
import React, { ChangeEvent, MouseEvent, useCallback, useEffect, useMemo, useState } from "react";
import Modal from "react-modal";
import SplitPane from "react-split-pane";
import "./external/react-splitpane.css";
import { CodeEditor, ExternalLink, Language } from "./components";
import { Spinner } from "./components";
import * as formatterWorker from "./FormatterWorker";
import "./Playground.css";
import { PluginInfo } from "./plugins";

Modal.setAppElement("#root");

export interface PlaygroundProps {
  configText: string;
  onConfigTextChanged: (text: string) => void;
  text: string;
  onTextChanged: (text: string) => void;
  formattedText: string;
  fileExtensions: string[];
  selectedPlugin: PluginInfo;
  plugins: PluginInfo[];
  onSelectPlugin: (plugin: PluginInfo) => void;
  isLoading: boolean;
  setPlugins: (plugins: PluginInfo[]) => void;
}

const defaultNewPluginJSON = JSON.stringify(
  {
    url: "https://plugins.dprint.dev/typescript-0.51.0.wasm",
    configSchemaUrl: "https://plugins.dprint.dev/schemas/typescript-v0.json",
    language: "typescript",
    // Get this directly from the wasm file
    fileExtensions: [".ts"],
  },
  null,
  2,
);

export function Playground({
  configText,
  onConfigTextChanged,
  text,
  onTextChanged,
  formattedText,
  fileExtensions,
  selectedPlugin,
  plugins,
  onSelectPlugin,
  isLoading,
  setPlugins,
}: PlaygroundProps) {
  const [scrollTop, setScrollTop] = useState(0);
  const [fileExtension, setFileExtension] = useState<string | undefined>(undefined);
  const [modalIsOpen, setIsOpen] = React.useState(false);
  const [newPluginJson, setNewPluginJson] = React.useState("");

  useEffect(() => {
    setFileExtension(fileExtensions[0]);
  }, [fileExtensions]);

  useEffect(() => {
    const timeout = setTimeout(() => {
      formatterWorker.formatText("file." + fileExtension ?? "ts", text);
    }, 250);

    return () => clearTimeout(timeout);
  }, [fileExtension, text]);

  useEffect(() => {
    const timeout = setTimeout(() => {
      let config;
      try {
        config = JSON.parse(configText);
        if (config.lineWidth == null) {
          config.lineWidth = 80;
        }
        formatterWorker.setConfig(config);
      } catch (err) {
        // ignore for now
      }
    }, 250);

    return () => clearTimeout(timeout);
  }, [configText]);

  const lineWidth = useMemo(() => {
    try {
      const lineWidth = parseInt(JSON.parse(configText).lineWidth, 10);
      if (!isNaN(lineWidth)) {
        return lineWidth;
      }
    } catch (err) {
      // ignore
    }
    return 80;
  }, [configText]);
  const onFileExtensionChange = useCallback((event: ChangeEvent<HTMLSelectElement>) => {
    setFileExtension(event.target.value);
  }, [setFileExtension]);

  const onNewPluginLoad = useCallback((event: MouseEvent<HTMLButtonElement>) => {
    setNewPluginJson(defaultNewPluginJSON);
    setIsOpen(true);
  }, [setNewPluginJson, setIsOpen]);

  const onCloseModal = useCallback((event: MouseEvent<HTMLButtonElement>) => {
    ow(newPluginJson, ow.string.nonEmpty);

    const pluginInfo = JSON.parse(newPluginJson) as PluginInfo;

    // TODO: replace this with schema validation
    ow(
      pluginInfo,
      ow.object.exactShape({
        url: ow.string.nonEmpty,
        configSchemaUrl: ow.string,
        language: ow.string.nonEmpty,
        fileExtensions: ow.array.ofType(ow.string.nonEmpty),
      }),
    );
    setIsOpen(false);

    setPlugins([
      ...plugins,
      pluginInfo,
    ]);
    onSelectPlugin(pluginInfo);
  }, [plugins, setPlugins, newPluginJson, setIsOpen, onSelectPlugin]);

  return (
    <div className="App">
      <Modal
        isOpen={modalIsOpen}
        onRequestClose={onCloseModal}
        contentLabel="Plugin ino"
      >
        {/* TODO: pass JSON schema */}
        <CodeEditor
          language={Language.Json}
          onChange={setNewPluginJson}
          text={newPluginJson}
        />
        <button onClick={onCloseModal}>Load plugin</button>
      </Modal>
      <SplitPane split="horizontal" defaultSize={53} allowResize={false}>
        <header className="appHeader">
          <h1 id="title">dprint - Playground</h1>
          <div id="headerRight">
            <a href="/overview">Overview</a>
            <a href="/playground">Playground</a>
            <a href="/sponsor">Sponsor</a>
            <ExternalLink url="https://github.com/dprint/dprint" text="View on GitHub" />
          </div>
        </header>
        <SplitPane
          split="vertical"
          minSize={50}
          defaultSize="50%"
          allowResize={true}
          pane1Style={{ overflowX: "hidden", overflowY: "hidden" }}
          pane2Style={{ overflowX: "hidden", overflowY: "hidden" }}
        >
          <SplitPane
            split="horizontal"
            allowResize={true}
            defaultSize="60%"
            pane1Style={{ overflowX: "hidden", overflowY: "hidden" }}
            pane2Style={{ overflowX: "hidden", overflowY: "hidden" }}
          >
            <div className="container">
              <div className="playgroundSubTitle">
                <div className="row">
                  <div className="column">
                    Plugin:
                  </div>
                  <div className="column">
                    <button onClick={onNewPluginLoad}>✍</button>
                  </div>
                  <div className="column" style={{ flex: 1, display: "flex" }}>
                    <select onChange={e => onSelectPlugin(plugins[e.target.selectedIndex])} style={{ flex: 1 }} value={selectedPlugin.url}>
                      {plugins.map((pluginInfo, i) => {
                        return <option key={i} value={pluginInfo.url}>
                          {pluginInfo.url}
                        </option>;
                      })}
                    </select>
                  </div>
                  <div className="column" style={{ display: "flex" }}>
                    <select value={fileExtension} onChange={onFileExtensionChange}>
                      {fileExtensions.map((ext, i) => <option key={i} value={ext}>{"."}{ext}</option>)}
                    </select>
                  </div>
                </div>
              </div>
              <CodeEditor
                language={selectedPlugin.language}
                onChange={onTextChanged}
                text={text}
                lineWidth={lineWidth}
                onScrollTopChange={setScrollTop}
                scrollTop={scrollTop}
              />
            </div>
            <div className="container">
              <div className="playgroundSubTitle">
                Configuration
              </div>
              <CodeEditor
                language={Language.Json}
                onChange={onConfigTextChanged}
                jsonSchemaUrl={selectedPlugin.configSchemaUrl}
                text={configText}
              />
            </div>
          </SplitPane>
          <div className="container">
            {isLoading ? <Spinner /> : <CodeEditor
              language={selectedPlugin.language}
              text={formattedText}
              readonly={true}
              lineWidth={lineWidth}
              onScrollTopChange={setScrollTop}
              scrollTop={scrollTop}
            />}
          </div>
        </SplitPane>
      </SplitPane>
    </div>
  );
}
