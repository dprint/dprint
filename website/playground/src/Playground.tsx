import React, { useState, useCallback, useMemo, useEffect, ChangeEvent } from "react";
import SplitPane from "react-split-pane";
import { CodeEditor, ExternalLink, Language } from "./components";
import { Spinner } from "./components";
import "./Playground.css";
import "./external/react-splitpane.css";
import { PluginInfo } from "./plugins";
import * as formatterWorker from "./FormatterWorker";

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
}

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
}: PlaygroundProps) {
    const [scrollTop, setScrollTop] = useState(0);
    const [fileExtension, setFileExtension] = useState<string | undefined>(undefined);

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

    return (
        <div className="App">
            <SplitPane split="horizontal" defaultSize={50} allowResize={false}>
                <header className="appHeader">
                    <h1 id="title">dprint - Playground</h1>
                    <div id="headerRight">
                        <a href="/overview">Overview</a>
                        <a href="/playground">Playground</a>
                        <a href="/pricing">Pricing</a>
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
                                language={Language.TypeScript}
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
                            language={Language.TypeScript}
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
