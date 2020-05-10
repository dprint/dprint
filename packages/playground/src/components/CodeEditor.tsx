import React from "react";
import ReactMonacoEditorForTypes from "react-monaco-editor";
import * as monacoEditorForTypes from "monaco-editor";
import { Spinner } from "./Spinner";

export interface CodeEditorProps {
    onChange?: (text: string) => void;
    text?: string;
    readonly?: boolean;
    lineWidth: number;
    scrollTop: number;
    onScrollTopChange: (scrollTop: number) => void;
}

export interface CodeEditorState {
    editorComponent: (typeof ReactMonacoEditorForTypes) | undefined | false;
}

export class CodeEditor extends React.Component<CodeEditorProps, CodeEditorState> {
    private editor: monacoEditorForTypes.editor.IStandaloneCodeEditor | undefined;

    constructor(props: CodeEditorProps) {
        super(props);
        this.state = {
            editorComponent: undefined,
        };
        this.editorDidMount = this.editorDidMount.bind(this);

        const reactMonacoEditorPromise = import("react-monaco-editor");
        import("monaco-editor").then(monacoEditor => {
            monacoEditor.languages.typescript.typescriptDefaults.setCompilerOptions({
                noLib: true,
                target: monacoEditor.languages.typescript.ScriptTarget.ESNext,
                allowNonTsExtensions: true,
            });
            monacoEditor.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
                noSyntaxValidation: true,
                noSemanticValidation: true,
            });
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

        return (
            <div id="codeEditor">
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
                language="typescript"
                onChange={text => this.props.onChange && this.props.onChange(text)}
                editorDidMount={this.editorDidMount}
                options={{
                    automaticLayout: true,
                    renderWhitespace: "all",
                    readOnly: this.props.readonly || false,
                    minimap: { enabled: false },
                    quickSuggestions: false,
                    rulers: [this.props.lineWidth],
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
    }

    private lastScrollTop = 0;
    private updateScrollTop() {
        if (this.editor == null || this.lastScrollTop === this.props.scrollTop)
            return;

        // todo: not sure how to not do this in the render method? I'm not a react/web person.
        setTimeout(() => {
            this.editor!.setScrollTop(this.props.scrollTop);
            this.lastScrollTop = this.props.scrollTop;
        }, 0);
    }
}
