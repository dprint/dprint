import { Node, SyntaxKind, JSONScanner, createScanner, NodeType } from "jsonc-parser";
import { parserHelpers, resolveNewLineKindFromText, PrintItemIterable, PrintItemKind, Signal, PrintItem, LoggingEnvironment } from "@dprint/core";
import { ResolvedJsoncConfiguration } from "../configuration";
import { throwError } from "../utils";

const { withIndent, parseJsLikeCommentLine } = parserHelpers;

export interface Context {
    fileText: string;
    log: (message: string) => void;
    warn: (message: string) => void;
    config: ResolvedJsoncConfiguration;
    scanner: JSONScanner; // for getting the comments, since they aren't in the AST
}

// const enum wasn't working for me, so using this workaround
const LocalSyntaxKind: {
    CommaToken: SyntaxKind.CommaToken;
    CommentLineTrivia: SyntaxKind.LineCommentTrivia;
    CommentBlockTrivia: SyntaxKind.BlockCommentTrivia;
    LineBreakTrivia: SyntaxKind.LineBreakTrivia;
    Trivia: SyntaxKind.Trivia;
    EOF: SyntaxKind.EOF;
} = {
    CommaToken: 5,
    CommentLineTrivia: 12,
    CommentBlockTrivia: 13,
    LineBreakTrivia: 14,
    Trivia: 15,
    EOF: 17
};

export interface ParseJsonFileOptions {
    file: Node | undefined;
    filePath: string;
    fileText: string;
    config: ResolvedJsoncConfiguration;
    environment: LoggingEnvironment;
}

export function* parseJsonFile(options: ParseJsonFileOptions): PrintItemIterable {
    const { file, filePath, fileText, config, environment } = options;
    const context: Context = {
        fileText,
        log: message => environment.log(`${message} (${filePath})`),
        warn: message => environment.warn(`${message} (${filePath})`),
        config,
        scanner: createScanner(fileText, false)
    };

    // this will be undefined when the file has no object node
    if (file == null) {
        yield* parseCommentsUpToPos({
            allowLeadingBlankLine: false,
            allowTrailingBlankLine: false,
            stopPos: fileText.length
        }, context);
    }
    else {
        yield* parseNode(file, context);
    }

    yield {
        kind: PrintItemKind.Condition,
        name: "endOfFileNewLine",
        condition: conditionContext => {
            return conditionContext.writerInfo.columnNumber > 0 || conditionContext.writerInfo.lineNumber > 0;
        },
        true: [Signal.NewLine]
    };
}

const parseObj: { [name in NodeType]: (node: Node, context: Context) => PrintItemIterable; } = {
    string: parseNodeAsIs,
    array: parseArray,
    boolean: parseNodeAsIs,
    "null": parseNodeAsIs,
    number: parseNodeAsIs,
    object: parseObject,
    property: parseProperty
};

interface ParseNodeOptions {
    /**
     * Inner parse useful for adding items at the beginning or end of the iterator
     * after leading comments and before trailing comments.
     */
    innerParse?(iterator: PrintItemIterable): PrintItemIterable;
}

function* parseNode(node: Node, context: Context, opts?: ParseNodeOptions): PrintItemIterable {
    yield* parseLeadingComments(node, context);

    const parseFunc = parseObj[node.type] || parseUnknownNode;
    const initialPrintItemIterable = parseFunc(node, context);
    const printItemIterator = opts && opts.innerParse ? opts.innerParse(initialPrintItemIterable) : initialPrintItemIterable;
    yield* printItemIterator;

    yield* parseTrailingComments(node, context);

    // get the trailing comments of the file
    if (node.parent == null) {
        yield* parseCommentsUpToPos({
            stopPos: context.fileText.length,
            allowLeadingBlankLine: true,
            allowTrailingBlankLine: false
        }, context);
    }
}

function* parseObject(node: Node, context: Context): PrintItemIterable {
    yield "{";
    yield* parseCommentsOnStartSameLine(node, context);
    yield* parseChildren(node, context);
    yield "}";
}

function* parseNodeAsIs(node: Node, context: Context): PrintItemIterable {
    yield {
        kind: PrintItemKind.RawString,
        text: context.fileText.substr(node.offset, node.length)
    };
}

function* parseArray(node: Node, context: Context): PrintItemIterable {
    yield "[";
    yield* parseCommentsOnStartSameLine(node, context);
    yield* parseChildren(node, context);
    yield "]";
}

function* parseProperty(node: Node, context: Context): PrintItemIterable {
    if (node.children == null || node.children.length !== 2)
        return throwError(`Expected the property node to have two children.`);

    yield* parseNode(node.children[0], context);
    yield ": ";
    yield* parseNode(node.children[1], context);
}

function* parseUnknownNode(node: Node, context: Context): PrintItemIterable {
    const nodeText = context.fileText.substr(node.offset, node.length);

    context.log(`"Not implemented node type": ${node.type} (${nodeText.substring(0, 100)})`);

    yield {
        kind: PrintItemKind.RawString,
        text: nodeText
    };
}

/* helpers */
function* parseChildren(node: Node, context: Context) {
    const wasLastCommentLine = context.scanner.getToken() === LocalSyntaxKind.CommentLineTrivia;
    const children = node.children;
    const innerComments = children == null || children.length === 0 ? getInnerComments(node.offset + node.length, context) : [];
    const multiLine = getUseMultipleLines();

    if (multiLine)
        yield Signal.NewLine;

    if (children == null || children.length === 0) {
        if (innerComments.length === 0)
            return;

        yield* withIndent(function*() {
            if (node.type === "object" && !multiLine)
                yield " ";

            for (let i = 0; i < innerComments.length; i++) {
                const comment = innerComments[i];
                if (comment.kind === LocalSyntaxKind.CommentBlockTrivia)
                    yield* parseCommentBlock(context.fileText.substr(comment.offset + 2, comment.length - 4));
                else if (comment.kind === LocalSyntaxKind.CommentLineTrivia)
                    yield* parseCommentLine(context.fileText.substr(comment.offset + 2, comment.length - 2));

                if (i < innerComments.length - 1) {
                    yield multiLine ? Signal.NewLine : Signal.SpaceOrNewLine;

                    if (multiLine && hasBlankLineAfterPos(comment.offset + comment.length, context))
                        yield Signal.NewLine;
                }
            }
        }());

        if (multiLine)
            yield Signal.NewLine;
        else if (node.type === "object")
            yield " ";

        return;
    }

    if (node.type === "object" && !multiLine)
        yield " ";

    yield* withIndent(function*() {
        for (let i = 0; i < children.length; i++) {
            const child = children[i];
            yield* parseNode(child, context, {
                innerParse: function*(iterator) {
                    yield* iterator;
                    if (i < children.length - 1)
                        yield ",";
                }
            });

            if (i < children.length - 1) {
                // skip the scanner past the comma token
                while (context.scanner.getToken() !== LocalSyntaxKind.CommaToken && context.scanner.getToken() !== LocalSyntaxKind.EOF)
                    context.scanner.scan();

                yield* parseCommentsOnSameLine(context);

                yield multiLine ? Signal.NewLine : Signal.SpaceOrNewLine;

                if (multiLine && hasBlankLineAfterPos(child.offset + child.length, context))
                    yield Signal.NewLine;
            }
        }

        if (multiLine)
            yield Signal.NewLine;
        else if (node.type === "object")
            yield " ";

        // -1 for going up to the } or ]
        yield* parseCommentsUpToPos({
            stopPos: node.offset + node.length - 1,
            allowLeadingBlankLine: true,
            allowTrailingBlankLine: false
        }, context);
    }());

    function getUseMultipleLines() {
        if (node.parent == null)
            return true; // always use multiple lines for the root node

        // check if should use multi-lines for inner comments
        if (node.children == null || node.children.length === 0) {
            if (wasLastCommentLine)
                return true;
            if (innerComments.length === 0)
                return false;
            else if (innerComments.some(c => c.kind === LocalSyntaxKind.CommentLineTrivia))
                return true;
            else {
                const lastComment = innerComments[innerComments.length - 1];
                return hasSeparatingNewLine(node.offset, lastComment.offset + lastComment.length, context);
            }
        }

        // check if the first child is on a new line
        const firstChildStart = node.children[0].offset;
        return hasSeparatingNewLine(node.offset, firstChildStart, context);
    }
}

/* comments */

function* parseLeadingComments(node: Node, context: Context) {
    const allowLeadingBlankLine = node.parent != null
        && node.parent.children != null
        && node.parent.children[0] !== node;

    yield* parseCommentsUpToPos({
        stopPos: node.offset,
        allowLeadingBlankLine,
        allowTrailingBlankLine: true
    }, context);
}

function* parseTrailingComments(node: Node, context: Context) {
    // keep the scanner in a non-unknown state
    while (context.scanner.getPosition() < node.offset + node.length)
        context.scanner.scan();

    yield* parseCommentsOnSameLine(context);
}

interface ParseCommentsUpToPosOptions {
    stopPos: number;
    allowLeadingBlankLine: boolean;
    allowTrailingBlankLine: boolean;
}

function* parseCommentsUpToPos(opts: ParseCommentsUpToPosOptions, context: Context): PrintItemIterable {
    const { stopPos, allowLeadingBlankLine, allowTrailingBlankLine } = opts;
    let lastCommentKind: SyntaxKind.LineCommentTrivia | SyntaxKind.BlockCommentTrivia | undefined = undefined;
    let lastLineBreakCount = 0;

    while (true) {
        const startPos = context.scanner.getPosition();
        const kind = context.scanner.scan();
        const endPos = context.scanner.getPosition();

        if (kind === LocalSyntaxKind.CommentLineTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos);
            yield* handleSpacing();
            yield* parseCommentLine(text);

            lastCommentKind = kind;
            lastLineBreakCount = 0;
        }
        else if (kind === LocalSyntaxKind.CommentBlockTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos - 2);
            yield* handleSpacing();
            yield* parseCommentBlock(text);

            lastCommentKind = kind;
            lastLineBreakCount = 0;
        }
        else if (kind === LocalSyntaxKind.LineBreakTrivia) {
            lastLineBreakCount++;
        }

        if (kind === LocalSyntaxKind.EOF || endPos >= stopPos) {
            if (allowTrailingBlankLine || lastCommentKind != null)
                yield* handleSpacing();
            break;
        }
    }

    function* handleSpacing(): PrintItemIterable {
        const canDoBlankLine = lastCommentKind != null || allowLeadingBlankLine;
        const shouldDoBlankLine = canDoBlankLine && lastLineBreakCount >= 2;

        if (shouldDoBlankLine) {
            if (lastCommentKind != null)
                yield Signal.NewLine;

            yield Signal.NewLine;
        }
        else if (lastCommentKind === LocalSyntaxKind.CommentBlockTrivia) {
            if (lastLineBreakCount === 0)
                yield " ";
            else
                yield Signal.NewLine;
        }
    }
}

function* parseCommentsOnStartSameLine(node: Node, context: Context) {
    // do not do this for the root node
    if (node.parent == null)
        return;

    // keep the scanner in a valid state and skip the opening token (ex. { or [)
    while (context.scanner.getPosition() < node.offset + 1)
        context.scanner.scan();

    yield* parseCommentsOnSameLine(context);
}

function parseCommentsOnSameLine(context: Context): PrintItemIterable {
    const { scanner } = context;
    const originalTokenPos = scanner.getTokenOffset();
    const comments: PrintItem[] = [];
    let lastTokenPos = originalTokenPos;

    while (true) {
        const kind = scanner.scan();
        const startPos = scanner.getTokenOffset();
        const endPos = scanner.getPosition();

        if (kind === LocalSyntaxKind.Trivia)
            continue;
        else if (kind === LocalSyntaxKind.CommentBlockTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos - 2);
            comments.push(" ");
            for (const item of parseCommentBlock(text))
                comments.push(item);
        }
        else if (kind === LocalSyntaxKind.CommentLineTrivia) {
            const text = context.fileText.substring(startPos + 2, endPos);
            comments.push(" ");
            for (const item of parseCommentLine(text))
                comments.push(item);
            return comments;
        }
        else if (kind === LocalSyntaxKind.LineBreakTrivia || kind === LocalSyntaxKind.EOF || kind === LocalSyntaxKind.CommaToken) {
            // reset the scanner to before this token
            scanner.setPosition(lastTokenPos);
            scanner.scan();

            return comments;
        }
        else {
            // reset the scanner
            scanner.setPosition(originalTokenPos);
            scanner.scan();

            return [];
        }

        lastTokenPos = startPos;
    }
}

function* parseCommentLine(commentValue: string): PrintItemIterable {
    yield parseJsLikeCommentLine(commentValue);
    yield Signal.ExpectNewLine;
}

function* parseCommentBlock(commentText: string): PrintItemIterable {
    yield "/*";
    yield {
        kind: PrintItemKind.RawString,
        text: commentText
    };
    yield "*/";
}

function hasSeparatingNewLine(startPos: number, endPos: number, context: Context) {
    for (let i = startPos; i < endPos; i++) {
        if (context.fileText[i] === "\n")
            return true;
    }
    return false;
}

function hasBlankLineAfterPos(startPos: number, context: Context) {
    const { scanner } = context;
    const lastTokenOffset = scanner.getTokenOffset();

    scanner.setPosition(startPos);

    try {
        let lineBreakCount = 0;
        while (true) {
            const kind = scanner.scan();
            if (kind === LocalSyntaxKind.Trivia)
                continue;
            if (kind === LocalSyntaxKind.LineBreakTrivia) {
                lineBreakCount++;
                if (lineBreakCount === 2)
                    return true;
                else
                    continue;
            }

            return false;
        }
    } finally {
        // restore to position
        scanner.setPosition(lastTokenOffset);
        scanner.scan();
    }
}

interface Comment {
    kind: SyntaxKind.LineCommentTrivia | SyntaxKind.BlockCommentTrivia;
    offset: number;
    length: number;
}

function getInnerComments(end: number, context: Context) {
    const scanner = context.scanner;
    const comments: Comment[] = [];

    while (true) {
        const kind = scanner.scan();
        if (kind === LocalSyntaxKind.CommentLineTrivia || kind === LocalSyntaxKind.CommentBlockTrivia) {
            comments.push({
                kind,
                offset: scanner.getTokenOffset(),
                length: scanner.getTokenLength()
            });
        }
        else if (scanner.getPosition() >= end) {
            break;
        }
    }

    return comments;
}
