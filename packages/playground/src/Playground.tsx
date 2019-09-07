import React from "react";
import SplitPane from "react-split-pane";
import { formatFileText, resolveConfiguration, LoggingEnvironment } from "@dprint/core";
import { TypeScriptPlugin, TypeScriptConfiguration } from "dprint-plugin-typescript";
import { CodeEditor, ConfigurationSelection, ExternalLink } from "./components";
import * as constants from "./constants";
import "./Playground.css";
import "./external/react-splitpane.css";

export interface PlaygroundState {
    text: string;
    formattedText: string;
    scrollTop: number;
    config: TypeScriptConfiguration;
}
const initialLineWidth = 80;
const environment: LoggingEnvironment = {
    error: () => {},
    log: () => {},
    warn: () => {}
};

export class Playground extends React.Component<{}, PlaygroundState> {
    constructor(props: {}) {
        super(props);

        const defaultConfig = this.getDefaultConfiguration();
        const initialText = getInitialText();
        const config: TypeScriptConfiguration = {
            lineWidth: defaultConfig.lineWidth,
            indentWidth: defaultConfig.indentWidth,
            useTabs: defaultConfig.useTabs,
            semiColons: defaultConfig["breakStatement.semiColon"],
            singleQuotes: defaultConfig.singleQuotes,
            trailingCommas: defaultConfig["tupleType.trailingCommas"],
            useBraces: defaultConfig["ifStatement.useBraces"],
            bracePosition: defaultConfig["arrowFunctionExpression.bracePosition"],
            singleBodyPosition: defaultConfig["ifStatement.singleBodyPosition"],
            nextControlFlowPosition: defaultConfig["ifStatement.nextControlFlowPosition"],
            forceMultiLineArguments: defaultConfig["callExpression.forceMultiLineArguments"],
            forceMultiLineParameters: defaultConfig["functionDeclaration.forceMultiLineParameters"],
            "enumDeclaration.memberSpacing": defaultConfig["enumDeclaration.memberSpacing"],
            "arrowFunctionExpression.useParentheses": defaultConfig["arrowFunctionExpression.useParentheses"]
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
                        <ExternalLink id={constants.css.viewOnGitHub.id} url="https://github.com/dsherret/dprint" text="View on GitHub" />
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
    }

    private lastUpdateTimeout: NodeJS.Timeout | undefined;
    private onTextChange(newText: string) {
        if (this.lastUpdateTimeout != null)
            clearTimeout(this.lastUpdateTimeout);

        this.setState({ text: newText });

        this.lastUpdateTimeout = setTimeout(() => {
            this.setState({ formattedText: this.getFormattedText() });
        }, 250);
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

    private getDefaultConfiguration() {
        return new TypeScriptPlugin({ lineWidth: initialLineWidth }).getConfiguration();
    }
}

function getInitialText() {
    return `// I quickly threw together this playground and will improve it in the future.
// In the meantime, this playground has all the defaults, except it uses a
// lineWidth of ${initialLineWidth} and not 120.

// In the future, I'll move this overview somewhere else...

/* ------- MULTILINE, HANGING INDENT, AND LINE WIDTH ------- */

// The following holds true for most nodes. Generally, nodes like
// call expressions will prefer to be on one line...

callExpression(argument1, argument2,
    argument3,    argument4);

// ...until you place the first arg on a different line...
call.expression(
    1, 2);

// ...or the statement exceeds the line width.
callExpression(argument1, argument2, argument3, argument4, argument5, argument6, argument7);

//If you don't like hanging, you can set the forceMultiLineArguments and
//forceMultiLineParameters configuration values to true, to force each
//param/arg to be on a newline when exceeding the line width.

/* ------- EXPLICIT NEWLINES ------- */

// For the most part, dprint allows you to place certain nodes like
// logical, binary, and property access expressions on different
// lines as you see fit. It does this because newlines can often
// convey meaning or grouping.
const mathResult = 1+2*6+
    moreMath*math
;
const binaryResult = true || false &&
possiblyTrue || (
 true&&false||maybeTrue);

expect(someFunctionCall(1  ,2))
    .to.    equal(5 );

// As seen above, placing a node on the next line after an open paren
// will indent the text within the parens.
const anotherMathResult = (
1 + 2)

// ...the same happens with statements like if statements.
if (
    someCondition && otherCondition) {

}

/* ------- BRACE POSITION ------- */

// By default, when an if or similar statement hangs, it will place the brace
// on a new line. This is to help separate the condition so it doesn't blur
// in with the first statement. You can disable this behaviour by setting the
// \`bracePosition\` setting to \`sameLine\` (defaults to \`newLineIfHanging\`).
if (someCondition && otherCondition || myCondition && yourCondition && myOtherCondition) {
    call();
}
else {
    console .   log(  'hello'
)}

// By default, dprint will add braces when they are missing except when the
// expression, is on the same line as the statement, but this can be configured
// with the \`useBraces\` setting.
// Note also that the position of the expression for single line bodies can
// by configured with the \`singleBodyPosition\` configuration.
if (true)
    statement;
if (true) statement;

/* ------- CLASS / INTERFACE HEADERS ------- */

// Classes/Interfaces will have their extends and implements clause put on
// a new line when they exceed the line width. Again, the brace position
// can be configured via the \`bracePosition\` option.
class MyClass extends SomeThing implements OtherThing, LoggerThing, FunctionalityThing, OtherOtherThing, ExtendingLineWidthTwiceThing {
}

/* ------- STATEMENT / MEMBER SPACING ------- */

function myFunction() {

        // Line breaks will be maintained, but not when they are


        // consecutive or if they are at the beginning or end of a block.

        return 5;

}

interface MyInterface {

    prop: string;


    otherProp: number;

    method(): number;
    otherMethod(): void;

}

/* ------- IGNORING A FILE ------- */

// Move the following comment to the top of the file:
// dprint-ignore-file

/* ------- IGNORING NODES ------- */

// dprint-ignore
const identity = [
    1, 0, 0, 0,
    0, 1, 0, 0,
    0, 0, 1, 0,
    0, 0, 0, 1
];`;
}
