import { Node, SyntaxKind, JSONScanner, createScanner, NodeType } from "jsonc-parser";
import { ResolvedConfiguration, resolveNewLineKindFromText } from "../../configuration";
import { PrintItemIterator, PrintItemKind, Signal } from "../../types";
import { throwError } from "../../utils";

export interface Context {
    file: Node;
    fileText: string;
    log: (message: string) => void;
    warn: (message: string) => void;
    config: ResolvedConfiguration;
    newlineKind: "\r\n" | "\n";
    scanner: JSONScanner; // for getting the comments, since they aren't in the AST
}

// const enum wasn't working for me, so using this workaround
const LocalSyntaxKind: {
    LineCommentTrivia: SyntaxKind.LineCommentTrivia;
    BlockCommentTrivia: SyntaxKind.BlockCommentTrivia;
    Trivia: SyntaxKind.Trivia;
    EOF: SyntaxKind.EOF;
} = {
    LineCommentTrivia: 12,
    BlockCommentTrivia: 13,
    Trivia: 15,
    EOF: 17
};

export function* parseJsonFile(file: Node, fileText: string, options: ResolvedConfiguration): PrintItemIterator {
    const context: Context = {
        file,
        fileText,
        log: message => console.log("[dprint]: " + message), // todo: use environment?
        warn: message => console.warn("[dprint]: " + message),
        config: options,
        newlineKind: options.newlineKind === "auto" ? resolveNewLineKindFromText(fileText) : options.newlineKind,
        scanner: createScanner(fileText, false)
    };

    yield* parseNode(file, context);
    yield {
        kind: PrintItemKind.Condition,
        name: "endOfFileNewLine",
        condition: conditionContext => {
            return conditionContext.writerInfo.columnNumber > 0 || conditionContext.writerInfo.lineNumber > 0;
        },
        true: [context.newlineKind]
    };
}

const parseObj: { [name in NodeType]: (node: Node, context: Context) => PrintItemIterator; } = {
    string: parseString,
    array: parseArray,
    boolean: parseBoolean,
    "null": parseNull,
    number: parseNumber,
    object: parseObject,
    property: parseProperty
};

function* parseNode(node: Node, context: Context): PrintItemIterator {
    yield* parseLeadingComments(node, context);

    const parseFunc = parseObj[node.type] || parseUnknownNode;
    yield* parseFunc(node, context);
}

function* parseObject(node: Node, context: Context): PrintItemIterator {
    yield "{";
    yield* parseCommentsOnSameLine(node, context);
    yield* parseChildren(node, context);
    yield "}";
}

function* parseString(node: Node, context: Context): PrintItemIterator {
    yield {
        kind: PrintItemKind.RawString,
        text: `"${node.value}"`
    };
}

function* parseArray(node: Node, context: Context): PrintItemIterator {
    yield "[";
    yield* parseCommentsOnSameLine(node, context);
    yield* parseChildren(node, context);
    yield "]";
}

function* parseBoolean(node: Node, context: Context): PrintItemIterator {
    yield node.value!.toString();
}

function* parseNull(node: Node, context: Context): PrintItemIterator {
    yield "null";
}

function* parseNumber(node: Node, context: Context): PrintItemIterator {
    yield node.value!.toString();
}

function* parseProperty(node: Node, context: Context): PrintItemIterator {
    if (node.children == null || node.children.length !== 2)
        return throwError(`Expected the property node to have two children.`);

    yield* parseNode(node.children[0], context);
    yield ": ";
    yield* parseNode(node.children[1], context);
}

function* parseUnknownNode(node: Node, context: Context): PrintItemIterator {
    const nodeText = context.fileText.substr(node.offset, node.length);

    context.log(`"Not implemented node type": ${node.type} (${nodeText.substring(0, 100)})`);

    yield {
        kind: PrintItemKind.RawString,
        text: nodeText
    };
}

/* helpers */
function* parseChildren(node: Node, context: Context) {
    const multiLine = getUseMultipleLines();

    if (multiLine)
        yield context.newlineKind;

    const children = node.children;
    if (children == null || children.length === 0)
        return;

    if (node.type === "object" && !multiLine)
        yield " ";

    yield* withIndent(function*() {
        for (let i = 0; i < children.length; i++) {
            yield* parseNode(children[i], context);

            if (i < children.length - 1) {
                yield ",";
                yield multiLine ? context.newlineKind : Signal.SpaceOrNewLine;
            }
        }
    }());

    if (multiLine)
        yield context.newlineKind;
    else if (node.type === "object")
        yield " ";

    function getUseMultipleLines() {
        if (node.parent == null)
            return true; // always use multiple lines for the root node

        if (node.children == null || node.children.length === 0)
            return false;

        const firstChildStart = node.children[0].offset;
        return hasSeparatingBlankLine(node.offset, firstChildStart, context);
    }
}

/* comments */

function* parseLeadingComments(node: Node, context: Context): PrintItemIterator {
    let lastCommentBlockEndPos: number | undefined;

    while (true) {
        const startPos = context.scanner.getPosition();
        const kind = context.scanner.scan();
        const endPos = context.scanner.getPosition();

        if (kind === LocalSyntaxKind.EOF || endPos > node.offset) {
            yield* handleSpacingIfLastWasCommentBlock(startPos);
            break;
        }

        if (kind === LocalSyntaxKind.LineCommentTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos);
            yield* handleSpacingIfLastWasCommentBlock(startPos);
            yield* parseCommentLine(text);
        }
        else if (kind === LocalSyntaxKind.BlockCommentTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos - 2);
            yield* handleSpacingIfLastWasCommentBlock(startPos);
            yield* parseCommentBlock(text);
            lastCommentBlockEndPos = endPos;
        }
    }

    function* handleSpacingIfLastWasCommentBlock(pos: number): PrintItemIterator {
        if (lastCommentBlockEndPos == null)
            return;

        const multiLine = hasSeparatingBlankLine(lastCommentBlockEndPos, pos, context);
        if (multiLine)
            yield context.newlineKind;
        else
            yield " ";

        lastCommentBlockEndPos = undefined;
    }
}

function* parseCommentsOnSameLine(node: Node, context: Context) {
    // do not do this for the root node
    if (node.parent == null)
        return;

    const startScannerPos = node.offset;
    context.scanner.setPosition(node.offset + 1); // skip the opening token (ex. { or [)

    while (true) {
        const startPos = context.scanner.getPosition();
        const kind = context.scanner.scan();
        const endPos = context.scanner.getPosition();

        if (kind === LocalSyntaxKind.EOF)
            break;
        else if (kind === LocalSyntaxKind.Trivia)
            continue;
        else if (kind === LocalSyntaxKind.LineCommentTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos);
            yield " ";
            yield* parseCommentLine(text);
            return;
        }
        else {
            context.scanner.setPosition(startScannerPos);
            return;
        }
    }
}

function* parseCommentLine(commentValue: string): PrintItemIterator {
    commentValue = commentValue.trim();

    yield "//";

    if (commentValue.length > 0)
        yield ` ${commentValue}`;

    yield Signal.ExpectNewLine;
}

function* parseCommentBlock(commentText: string): PrintItemIterator {
    yield "/*";
    yield {
        kind: PrintItemKind.RawString,
        text: commentText
    };
    yield "*/";
}

// todo: extract out and reuse with others
function* withIndent(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartIndent;
    yield* item;
    yield Signal.FinishIndent;
}

function hasSeparatingBlankLine(startPos: number, endPos: number, context: Context) {
    for (let i = startPos; i < endPos; i++) {
        if (context.fileText[i] === "\n")
            return true;
    }
    return false;
}
