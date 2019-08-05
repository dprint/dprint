import React from "react";
import ReactMonacoEditorForTypes from "react-monaco-editor";
import * as monacoEditorForTypes from "monaco-editor";
import { Spinner } from "./Spinner";
import { css as cssConstants } from "../constants";

export interface CodeEditorProps {
    onChange?: (text: string) => void;
    text?: string;
    readonly?: boolean;
    lineWidth: number;
}

export interface CodeEditorState {
    editorComponent: (typeof ReactMonacoEditorForTypes) | undefined | false;
}

export class CodeEditor extends React.Component<CodeEditorProps, CodeEditorState> {
    private editor: monacoEditorForTypes.editor.IStandaloneCodeEditor | undefined;

    constructor(props: CodeEditorProps) {
        super(props);
        this.state = {
            editorComponent: undefined
        };
        this.editorDidMount = this.editorDidMount.bind(this);

        const reactMonacoEditorPromise = import("react-monaco-editor");
        import("monaco-editor").then(monacoEditor => {
            monacoEditor.languages.typescript.typescriptDefaults.setCompilerOptions({
                noLib: true,
                target: monacoEditor.languages.typescript.ScriptTarget.ESNext
            });
            monacoEditor.languages.typescript.typescriptDefaults.setDiagnosticsOptions({
                noSyntaxValidation: true,
                noSemanticValidation: true
            });
            monacoEditor.editor.defineTheme("dprint-theme", {
                base: "vs-dark",
                inherit: true,
                rules: [],
                colors: {
                    "editorRuler.foreground": "#283430"
                }
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
        return (
            <div id={cssConstants.codeEditor.id}>
                <div id={cssConstants.codeEditor.containerId}>
                    {this.getEditor()}
                </div>
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
                    rulers: [this.props.lineWidth - 1]
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
                    lineNumber: 1
                });
            }
        });
    }
}
