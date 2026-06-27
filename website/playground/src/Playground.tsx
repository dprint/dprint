import type { FileMatchingInfo, PluginInfo } from "@dprint/formatter";
import JSON5 from "json5";
import React, { ChangeEvent, useCallback, useEffect, useMemo, useState } from "react";
import { CodeEditor } from "./components";
import { Spinner } from "./components";
import * as formatterWorker from "./FormatterWorker";
import { getLanguageFromPluginUrl, getPluginShortNameFromPluginUrl } from "./plugins";

import "./Playground.css";

// default accent (Slate) from the design; kept as a variable so it's easy to retheme.
const ACCENT = "#8b93a1";

export interface PlaygroundProps {
  configText: string;
  onConfigTextChanged: (text: string) => void;
  text: string;
  onTextChanged: (text: string) => void;
  fileExtension: string;
  onFileExtensionChanged: (ext: string) => void;
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
  fileExtension,
  onFileExtensionChanged,
  formattedText,
  fileMatchingInfo,
  selectedPluginUrl,
  selectedPluginInfo,
  pluginUrls,
  onSelectPluginUrl,
  isLoading,
}: PlaygroundProps) {
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    if (fileMatchingInfo.fileExtensions.length > 0) {
      if (fileExtension == null || !fileMatchingInfo.fileExtensions.includes(fileExtension)) {
        onFileExtensionChanged(fileMatchingInfo.fileExtensions[0]);
      }
    }
  }, [fileMatchingInfo, fileExtension, onFileExtensionChanged]);

  useEffect(() => {
    const timeout = setTimeout(() => {
      formatterWorker.formatText("file." + (fileExtension ?? "ts"), text);
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
    onFileExtensionChanged(event.target.value);
  }, [onFileExtensionChanged]);

  const editorLanguage = getLanguageFromPluginUrl(selectedPluginUrl) ?? "plaintext";
  const langLabel = getPluginShortNameFromPluginUrl(selectedPluginUrl) ?? "plugin";

  const onSelectPlugin = useCallback((event: ChangeEvent<HTMLSelectElement>) => {
    if (event.target.selectedIndex >= pluginUrls.length) {
      const url = prompt("Please provide a Wasm plugin url:", "");
      if (url != null && url.trim().length > 0) {
        onSelectPluginUrl(url);
      } else {
        event.preventDefault();
      }
    } else {
      onSelectPluginUrl(pluginUrls[event.target.selectedIndex]);
    }
  }, [pluginUrls, onSelectPluginUrl]);

  return (
    <div id="App" style={{ "--accent": ACCENT } as React.CSSProperties}>
      <nav id="AppNav">
        <div className="navInner">
          <a className="brand" href="/">dprint</a>
          <div className="navLinks">
            <a href="/overview">Overview</a>
            <a className="active" href="/playground">Playground</a>
            <a href="/sponsor">Sponsor</a>
            <a className="ghButton" href="https://github.com/dprint/dprint" rel="noopener noreferrer">
              GitHub <span className="ghArrow">↗</span>
            </a>
          </div>
        </div>
      </nav>

      <div className="page">
        <header className="pageHeader">
          <div className="eyebrow">// playground</div>
          <h1>Try the formatter</h1>
          <p>Paste code, pick a plugin, and format it live in the browser using the real Wasm formatter.</p>
        </header>

        <div className="toolbar">
          <div className="toolbarGroup">
            <span className="toolbarLabel">plugin</span>
            <select className="control" value={selectedPluginUrl} onChange={onSelectPlugin}>
              {pluginUrls.map((pluginUrl, i) => (
                <option key={i} value={pluginUrl}>
                  {getPluginShortNameFromPluginUrl(pluginUrl) ?? pluginUrl}
                </option>
              ))}
              <option key="custom">Custom…</option>
            </select>
            <select className="control" value={fileExtension} onChange={onFileExtensionChange}>
              {fileMatchingInfo.fileExtensions.map((ext, i) => <option key={i} value={ext}>.{ext}</option>)}
            </select>
          </div>
          <div className="toolbarGroup">
            <button className="btn btnGhost" onClick={() => onTextChanged("")}>Reset</button>
            <button
              className="btn btnPrimary"
              onClick={() => formatterWorker.formatText("file." + (fileExtension ?? "ts"), text)}
            >
              Format <span className="btnArrow">▸</span>
            </button>
          </div>
        </div>

        <div className="panes">
          <div className="leftCol">
            <section className="pane inputPane">
              <div className="paneHeader">
                <span className="paneLabel">Input</span>
                <span className="paneMeta">{langLabel}</span>
              </div>
              <div className="paneBody">
                <CodeEditor
                  language={editorLanguage}
                  onChange={onTextChanged}
                  text={text}
                  lineWidth={lineWidth}
                  onScrollTopChange={setScrollTop}
                  scrollTop={scrollTop}
                />
              </div>
            </section>

            <section className="pane configPane">
              <div className="paneHeader">
                <span className="paneLabel">Config</span>
                <span className="paneMeta">json</span>
              </div>
              <div className="paneBody">
                <CodeEditor
                  language={"json"}
                  onChange={onConfigTextChanged}
                  jsonSchemaUrl={selectedPluginInfo?.configSchemaUrl}
                  text={configText}
                />
              </div>
            </section>
          </div>

          <section className="pane outputPane">
            <div className="paneHeader">
              <span className="paneLabel">Output</span>
              <span className="paneMeta" style={{ color: isLoading ? "#5b626d" : "#98c379" }}>
                {isLoading ? "loading…" : "formatted ✓"}
              </span>
            </div>
            <div className="paneBody">
              {isLoading ? <Spinner backgroundColor="#181a1e" /> : (
                <CodeEditor
                  language={editorLanguage}
                  text={formattedText}
                  readonly={true}
                  lineWidth={lineWidth}
                  onScrollTopChange={setScrollTop}
                  scrollTop={scrollTop}
                />
              )}
            </div>
          </section>
        </div>

        <div className="tip">Tip: edit the code on the left and the formatted output updates live with your config.</div>
      </div>
    </div>
  );
}
