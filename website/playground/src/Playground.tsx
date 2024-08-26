import type { FileMatchingInfo, PluginInfo } from "@dprint/formatter";
import { Allotment } from "allotment";
import JSON5 from "json5";
import React, { ChangeEvent, useCallback, useEffect, useMemo, useState } from "react";
import { CodeEditor, ExternalLink } from "./components";
import { Spinner } from "./components";
import "./external/react-splitpane.css";
import * as formatterWorker from "./FormatterWorker";
import { getLanguageFromPluginUrl } from "./plugins";

import "./Playground.css";
import "allotment/dist/style.css";

export interface PlaygroundProps {
  configText: string;
  onConfigTextChanged: (text: string) => void;
  text: string;
  onTextChanged: (text: string) => void;
  formattedText: string;
  selectedPluginInfo: PluginInfo;
  fileMatchingInfo: FileMatchingInfo;
  selectedPluginUrl: string;
  pluginUrls: string[];
  onSelectPluginUrl: (pluginUrl: string) => void;
  isLoading: boolean;
}

export function Playground({
  configText,
  onConfigTextChanged,
  text,
  onTextChanged,
  formattedText,
  fileMatchingInfo,
  selectedPluginUrl,
  selectedPluginInfo,
  pluginUrls,
  onSelectPluginUrl,
  isLoading,
}: PlaygroundProps) {
  const [scrollTop, setScrollTop] = useState(0);
  const [fileExtension, setFileExtension] = useState<string | undefined>(undefined);

  useEffect(() => {
    if (fileMatchingInfo.fileExtensions.length > 0) {
      if (fileExtension == null || !fileMatchingInfo.fileExtensions.includes(fileExtension)) {
        setFileExtension(fileMatchingInfo.fileExtensions[0]);
      }
    }
  }, [fileMatchingInfo, fileExtension]);

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
        config = JSON5.parse(configText);
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
      const lineWidth = parseInt(JSON5.parse(configText).lineWidth, 10);
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

  return (
    <div id="App">
      <header id="AppHeader">
        <h1 id="title">dprint - Playground</h1>
        <div id="headerRight">
          <a href="/overview">Overview</a>
          <a href="/playground">Playground</a>
          <a href="/sponsor">Sponsor</a>
          <ExternalLink url="https://github.com/dprint/dprint" text="View on GitHub" />
        </div>
      </header>
      <div id="AppBody">
        <Allotment>
          <Allotment.Pane preferredSize="50%">
            <Allotment vertical={true}>
              <div className="container">
                <div className="playgroundSubTitle">
                  <div className="row">
                    <div className="column">
                      Plugin:
                    </div>
                    <div className="column" style={{ flex: 1, display: "flex" }}>
                      <select
                        onChange={e => {
                          if (e.target.selectedIndex >= pluginUrls.length) {
                            let url = prompt("Please provide a Wasm plugin url:", "");
                            if (url != null && url.trim().length > 0) {
                              onSelectPluginUrl(url);
                            } else {
                              e.preventDefault();
                            }
                          } else {
                            onSelectPluginUrl(pluginUrls[e.target.selectedIndex]);
                          }
                        }}
                        style={{ flex: 1 }}
                        value={selectedPluginUrl}
                      >
                        {pluginUrls.map((pluginUrl, i) => {
                          return (
                            <option key={i} value={pluginUrl}>
                              {pluginUrl}
                            </option>
                          );
                        })}
                        <option key="custom">Custom</option>
                      </select>
                    </div>
                    <div className="column" style={{ display: "flex" }}>
                      <select value={fileExtension} onChange={onFileExtensionChange}>
                        {fileMatchingInfo.fileExtensions.map((ext, i) => <option key={i} value={ext}>{"."}{ext}</option>)}
                      </select>
                    </div>
                  </div>
                </div>
                <CodeEditor
                  language={getLanguageFromPluginUrl(selectedPluginUrl) ?? "plaintext"}
                  onChange={onTextChanged}
                  text={text}
                  lineWidth={lineWidth}
                  onScrollTopChange={setScrollTop}
                  scrollTop={scrollTop}
                />
              </div>
              <Allotment.Pane preferredSize="40%">
                <div className="container">
                  <div className="playgroundSubTitle">
                    Configuration
                  </div>
                  <CodeEditor
                    language={"json"}
                    onChange={onConfigTextChanged}
                    jsonSchemaUrl={selectedPluginInfo?.configSchemaUrl}
                    text={configText}
                  />
                </div>
              </Allotment.Pane>
            </Allotment>
          </Allotment.Pane>
          <div className="container">
            {isLoading ? <Spinner /> : (
              <CodeEditor
                language={getLanguageFromPluginUrl(selectedPluginUrl) ?? "plaintext"}
                text={formattedText}
                readonly={true}
                lineWidth={lineWidth}
                onScrollTopChange={setScrollTop}
                scrollTop={scrollTop}
              />
            )}
          </div>
        </Allotment>
      </div>
    </div>
  );
}
