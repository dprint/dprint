import React from "react";
import type ReactMonacoEditorForTypes from "react-monaco-editor";
import type * as monacoEditorForTypes from "monaco-editor";
import { Spinner } from "./Spinner";

export interface CodeEditorProps {
    onChange?: (text: string) => void;
    text?: string;
    readonly?: boolean;
    lineWidth?: number;
    scrollTop?: number;
    jsonSchemaUrl?: string;
    onScrollTopChange?: (scrollTop: number) => void;
    language: Language;
}

export enum Language {
    TypeScript = "typescript",
    Json = "json",
}

export interface CodeEditorState {
    editorComponent: (typeof ReactMonacoEditorForTypes) | undefined | false;
}

export class CodeEditor extends React.Component<CodeEditorProps, CodeEditorState> {
    private editor: monacoEditorForTypes.editor.IStandaloneCodeEditor | undefined;
    private monacoEditor: typeof monacoEditorForTypes | undefined;
    private outerContainerRef = React.createRef<HTMLDivElement>();

    constructor(props: CodeEditorProps) {
        super(props);
        this.state = {
            editorComponent: undefined,
        };
        this.editorDidMount = this.editorDidMount.bind(this);

        const reactMonacoEditorPromise = import("react-monaco-editor");
        import("monaco-editor").then(monacoEditor => {
            this.monacoEditor = monacoEditor;
            if (this.props.language === Language.TypeScript) {
                monacoEditor.languages.typescript.typescriptDefaults.setCompilerOptions({
                    noLib: true,
                    target: monacoEditor.languages.typescript.ScriptTarget.ESNext,
                    allowNonTsExtensions: true,
                });
                monacoEditor.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
                    noSyntaxValidation: true,
                    noSemanticValidation: true,
                });
            }

            monacoEditor.editor.defineTheme("dprint-theme", {
                base: "vs-dark",
                inherit: true,
                rules: [],
                colors: {
                    "editorRuler.foreground": "#283430",
                },
            });

            reactMonacoEditorPromise.then(editor => {
                this.setState({ editorComponent: editor.default });
            }).catch(err => {
                console.log(err);
                this.setState({ editorComponent: false });
            });
        }).catch(err => {
            console.log(err);
            this.setState({ editorComponent: false });
        });
    }

    render() {
        this.updateScrollTop();
        this.updateJsonSchema();

        return (
            <div className="codeEditor" ref={this.outerContainerRef}>
                {this.getEditor()}
            </div>
        );
    }

    private getEditor() {
        if (this.state.editorComponent == null)
            return <Spinner backgroundColor="#1e1e1e" />;
        if (this.state.editorComponent === false)
            return <div className={"errorMessage"}>Error loading code editor. Please refresh the page to try again.</div>;

        return (
            <this.state.editorComponent
                width="100%"
                height="100%"
                value={this.props.text}
                theme="dprint-theme"
                language={this.props.language}
                onChange={text => this.props.onChange && this.props.onChange(text)}
                editorDidMount={this.editorDidMount}
                options={{
                    automaticLayout: false,
                    renderWhitespace: "all",
                    readOnly: this.props.readonly || false,
                    minimap: { enabled: false },
                    quickSuggestions: false,
                    rulers: this.props.lineWidth == null ? [] : [this.props.lineWidth],
                }}
            />
        );
    }

    private editorDidMount(editor: monacoEditorForTypes.editor.IStandaloneCodeEditor) {
        this.editor = editor;

        this.editor.onDidChangeModelContent(() => {
            if (this.props.readonly) {
                this.editor!.setPosition({
                    column: 1,
                    lineNumber: 1,
                });
            }
        });

        this.editor.onDidScrollChange(e => {
            if (e.scrollTopChanged && this.props.onScrollTopChange)
                this.props.onScrollTopChange(e.scrollTop);
        });

        // manually refresh the layout of the editor (lightweight compared to monaco editor)
        let lastHeight = 0;
        let lastWidth = 0;
        setInterval(() => {
            const containerElement = this.outerContainerRef.current;
            if (containerElement == null)
                return;

            const width = containerElement.offsetWidth;
            const height = containerElement.offsetHeight;
            if (lastHeight === height && lastWidth === width)
                return;

            editor.layout();

            lastHeight = height;
            lastWidth = width;
        }, 500);
    }

    private lastScrollTop = 0;
    private updateScrollTop() {
        if (this.editor == null || this.lastScrollTop === this.props.scrollTop)
            return;

        // todo: not sure how to not do this in the render method? I'm not a react/web person.
        setTimeout(() => {
            if (this.props.scrollTop != null) {
                this.editor!.setScrollTop(this.props.scrollTop);
                this.lastScrollTop = this.props.scrollTop;
            }
        }, 0);
    }

    private updateJsonSchema() {
        if (this.monacoEditor != null && this.props.jsonSchemaUrl != null) {
            if (this.monacoEditor.languages.json.jsonDefaults.diagnosticsOptions.schemas?.[0]?.uri !== this.props.jsonSchemaUrl) {
                this.monacoEditor.languages.json.jsonDefaults.setDiagnosticsOptions({
                    validate: true,
                    allowComments: true,
                    enableSchemaRequest: true,
                    schemas: [{
                        uri: this.props.jsonSchemaUrl,
                        fileMatch: ["*"],
                    }],
                });
            }
        }
    }
}
