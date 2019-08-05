import React from "react";
import SplitPane from "react-split-pane";
import { formatFileText, resolveConfiguration } from "dprint";
import { CodeEditor, ExternalLink } from "./components";
import * as constants from "./constants";
import "./Playground.css";

export interface PlaygroundState {
    text: string;
    formattedText: string;
}

const config = resolveConfiguration({
    lineWidth: 80
}).config;

export class Playground extends React.Component<{}, PlaygroundState> {
    constructor(props: {}) {
        super(props);

        const initialText = getInitialText();
        this.state = {
            text: initialText,
            formattedText: this.formatText(initialText)
        };

        this.onTextChange = this.onTextChange.bind(this);
    }

    render() {
        return (
            <div className="App">
                <SplitPane split="horizontal" defaultSize={50} allowResize={false}>
                    <header className="App-header">
                        <h2 id="title">dprint - Playground</h2>
                        <ExternalLink id={constants.css.viewOnGitHub.id} url="https://github.com/dsherret/dprint" text="View on GitHub" />
                    </header>
                    <SplitPane split="vertical" minSize={50} defaultSize="50%">
                        <CodeEditor onChange={this.onTextChange} text={this.state.text} lineWidth={config["typescript.lineWidth"]} />
                        <CodeEditor text={this.state.formattedText} readonly={true} lineWidth={config["typescript.lineWidth"]} />
                    </SplitPane>
                </SplitPane>
            </div>
        );
    }

    private lastUpdateTimeout: NodeJS.Timeout | undefined;
    onTextChange(newText: string) {
        if (this.lastUpdateTimeout != null)
            clearTimeout(this.lastUpdateTimeout);

        this.setState({ text: newText });

        this.lastUpdateTimeout = setTimeout(() => {
            this.setState({
                formattedText: this.formatText(newText)
            });
        }, 250);
    }

    formatText(text: string) {
        try {
            return formatFileText("file.ts", text, config);
        } catch (err) {
            return err.toString();
        }
    }
}

function getInitialText() {
    return `// I quickly threw together this playground. I'll add configuration here
// in the future. In the meantime, this playground has all the defaults,
// except it uses a lineWidth of ${config["typescript.lineWidth"]} and not 120.

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

//If you don't like hanging, there is
//configuration coming in issue #14 to force newlines. Until then, place
//the first arg on a different line as the open paren, as shown above.

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

// By default, dprint will maintain the brace behaviour, but this can be
// configured with the \`useBraces\` setting. The \`preferNone\` option is
// my favourite as it will add braces if the header or statement is hanging
// or, in the case of the last control flow statement (ex. \`else\`), it will
// add braces to that if the previous control flow statement required braces
// in order to prevent dangling else/else if statements. You may want to use
// the \`always\` option though.
if (true)
    statement;

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
/* dprint:ignoreFile */`;
}
