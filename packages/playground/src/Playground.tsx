import React from "react";
import SplitPane from "react-split-pane";
import { formatFileText, resolveConfiguration, LoggingEnvironment } from "@dprint/core";
import { TypeScriptPlugin, TypeScriptConfiguration } from "dprint-plugin-typescript";
import { CodeEditor, ConfigurationSelection, ExternalLink } from "./components";
import { UrlSaver } from "./utils";
import "./Playground.css";
import "./external/react-splitpane.css";

export interface PlaygroundState {
    text: string;
    formattedText: string;
    scrollTop: number;
    config: TypeScriptConfiguration;
}
const initialLineWidth = 80;
const urlSaver = new UrlSaver();
const environment: LoggingEnvironment = {
    error: () => {},
    log: () => {},
    warn: () => {}
};

export class Playground extends React.Component<{}, PlaygroundState> {
    constructor(props: {}) {
        super(props);

        const { text: initialText, config: initialUnresolvedConfig } = urlSaver.getUrlInfo();
        const initialConfig = this.getResolvedConfiguration(initialUnresolvedConfig);
        const config: TypeScriptConfiguration = {
            lineWidth: initialConfig.lineWidth,
            indentWidth: initialConfig.indentWidth,
            useTabs: initialConfig.useTabs,
            semiColons: initialConfig["breakStatement.semiColon"],
            singleQuotes: initialConfig.singleQuotes,
            trailingCommas: initialConfig["tupleType.trailingCommas"],
            useBraces: initialConfig["ifStatement.useBraces"],
            bracePosition: initialConfig["arrowFunctionExpression.bracePosition"],
            singleBodyPosition: initialConfig["ifStatement.singleBodyPosition"],
            nextControlFlowPosition: initialConfig["ifStatement.nextControlFlowPosition"],
            forceMultiLineArguments: initialConfig["callExpression.forceMultiLineArguments"],
            forceMultiLineParameters: initialConfig["functionDeclaration.forceMultiLineParameters"],
            "enumDeclaration.memberSpacing": initialConfig["enumDeclaration.memberSpacing"],
            "arrowFunctionExpression.useParentheses": initialConfig["arrowFunctionExpression.useParentheses"]
        };

        this.state = {
            text: initialText,
            formattedText: this.formatText(initialText, config),
            scrollTop: 0,
            config
        };

        this.onConfigUpdate = this.onConfigUpdate.bind(this);
        this.onTextChange = this.onTextChange.bind(this);
        this.onScrollTopChange = this.onScrollTopChange.bind(this);
    }

    render() {
        return (
            <div className="App">
                <SplitPane split="horizontal" defaultSize={50} allowResize={false}>
                    <header className="appHeader">
                        <h1 id="title">dprint - Playground</h1>
                        <div id="headerRight">
                            <a href="/">Overview</a>
                            <a href="/playground">Playground</a>
                            <ExternalLink url="https://github.com/dsherret/dprint" text="View on GitHub" />
                        </div>
                    </header>
                    {/* Todo: re-enable resizing, but doesn't seem to work well with monaco editor on
                    the right side as it won't reduce its width after being expanded. */}
                    <SplitPane split="vertical" minSize={50} defaultSize={200} allowResize={false}>
                        <ConfigurationSelection
                            config={this.state.config}
                            onUpdateConfig={this.onConfigUpdate}
                        />
                        <SplitPane split="vertical" minSize={50} defaultSize="50%" allowResize={false}>
                            <CodeEditor
                                onChange={this.onTextChange}
                                text={this.state.text}
                                lineWidth={this.state.config.lineWidth || initialLineWidth}
                                onScrollTopChange={this.onScrollTopChange}
                                scrollTop={this.state.scrollTop}
                            />
                            <CodeEditor
                                text={this.state.formattedText}
                                readonly={true}
                                lineWidth={this.state.config.lineWidth || initialLineWidth}
                                onScrollTopChange={this.onScrollTopChange}
                                scrollTop={this.state.scrollTop}
                            />
                        </SplitPane>
                    </SplitPane>
                </SplitPane>
            </div>
        );
    }

    private onConfigUpdate(config: TypeScriptConfiguration) {
        this.setState({ config, formattedText: this.getFormattedText(config) });
        this.updateUrl({ text: this.state.text, config });
    }

    private lastUpdateTimeout: NodeJS.Timeout | undefined;
    private onTextChange(newText: string) {
        if (this.lastUpdateTimeout != null)
            clearTimeout(this.lastUpdateTimeout);

        this.setState({ text: newText });

        this.lastUpdateTimeout = setTimeout(() => {
            this.setState({ formattedText: this.getFormattedText() });
            this.updateUrl({ text: newText, config: this.state.config });
        }, 250);
    }

    private updateUrl(urlInfo: { text: string; config: TypeScriptConfiguration; }) {
        urlSaver.updateUrl(urlInfo);
    }

    private getFormattedText(config?: TypeScriptConfiguration) {
        return this.formatText(this.state.text, config || this.state.config);
    }

    private onScrollTopChange(scrollTop: number) {
        this.setState({ scrollTop });
    }

    private formatText(text: string, typeScriptConfig: TypeScriptConfiguration) {
        try {
            const typeScriptPlugin = new TypeScriptPlugin(typeScriptConfig);
            const config = resolveConfiguration({}).config;
            typeScriptPlugin.initialize({
                environment,
                globalConfig: config
            });

            return formatFileText({
                filePath: "/file.ts",
                fileText: text,
                plugins: [typeScriptPlugin]
            });
        } catch (err) {
            return err.toString();
        }
    }

    private getResolvedConfiguration(config: TypeScriptConfiguration) {
        try {
            return new TypeScriptPlugin(config).getConfiguration();
        } catch (err) {
            console.error(err);
            return new TypeScriptPlugin({ lineWidth: 80 }).getConfiguration();
        }
    }
}
