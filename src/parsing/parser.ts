import * as babel from "@babel/types";
import { PrintItem, PrintItemKind, Group, Behaviour, Unknown, PrintItemIterator, Condition, Info, ResolveConditionContext } from "../types";
import { assertNever, removeStringIndentation, isPrintItemIterator } from "../utils";

interface Context {
    file: babel.File,
    fileText: string;
    log: (message: string) => void;
    options: ParseOptions;
    handledComments: Set<babel.Comment>;
    /** This is used to queue up the next item on the parent stack. */
    currentNode: babel.Node;
    parentStack: babel.Node[];
}

export interface ParseOptions {
    newLineKind: "\r\n" | "\n";
    semiColons: boolean;
    singleQuotes: boolean;
}

export function parseFile(file: babel.File, fileText: string, options: ParseOptions): Group {
    const context: Context = {
        file,
        fileText,
        log: message => console.log("[dprint]: " + message),
        options,
        handledComments: new Set<babel.Comment>(),
        currentNode: file,
        parentStack: []
    };

    return {
        kind: PrintItemKind.Group,
        items: function*(): PrintItemIterator {
            yield parseNode(file.program, context);
            yield {
                kind: PrintItemKind.Condition,
                name: "endOfFileNewLine",
                condition: conditionContext => {
                    return conditionContext.writerInfo.columnNumber > 0 || conditionContext.writerInfo.lineNumber > 0;
                },
                true: [context.options.newLineKind]
            };
        }()
    };
}

const parseObj: { [name: string]: (node: any, context: Context) => PrintItem | PrintItemIterator; } = {
    /* file */
    "Program": parseProgram,
    /* common */
    "BlockStatement": parseBlockStatement,
    "Identifier": parseIdentifier,
    /* declarations */
    "ExportNamedDeclaration": parseExportNamedDeclaration,
    "FunctionDeclaration": parseFunctionDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    "ImportDeclaration": parseImportDeclaration,
    /* statements */
    "ExpressionStatement": parseExpressionStatement,
    "IfStatement": parseIfStatement,
    /* expressions */
    "CallExpression": parseCallExpression,
    /* imports */
    "ImportDefaultSpecifier": parseImportDefaultSpecifier,
    "ImportNamespaceSpecifier": parseImportNamespaceSpecifier,
    "ImportSpecifier": parseImportSpecifier,
    /* literals */
    "StringLiteral": parseStringLiteral,
    "BooleanLiteral": parseBooleanLiteral,
    /* keywords */
    "TSStringKeyword": () => "string",
    "TSNumberKeyword": () => "number",
    "TSBooleanKeyword": () => "boolean",
    "TSAnyKeyword": () => "any",
    "TSUnknownKeyword": () => "unknown",
    "TSObjectKeyword": () => "object",
    "ThisExpression": () => "this",
    /* types */
    "TSTypeParameter": parseTypeParameter,
    "TSUnionType": parseUnionType,
};

function parseNode(node: babel.Node | null, context: Context): Group {
    if (node == null) {
        // todo: make this a static object? (with Object.freeze?)
        return {
            kind: PrintItemKind.Group,
            items: []
        };
    }

    context.parentStack.push(context.currentNode);
    context.currentNode = node;

    const parseFunc = parseObj[node!.type] || parseUnknownNode;
    const printItem = parseFunc(node, context);
    const group: Group = {
        kind: PrintItemKind.Group,
        items: getWithComments(node, printItem, context)
    };

    // replace the past item
    context.currentNode = context.parentStack.pop()!;

    return group;
}

/* file */
function* parseProgram(node: babel.Program, context: Context): PrintItemIterator {
    yield* parseStatements(node, context);
}

/* common */

function* parseBlockStatement(node: babel.BlockStatement, context: Context): PrintItemIterator {
    yield "{";
    yield* getFirstLineTrailingComments();
    yield context.options.newLineKind;
    yield* withIndent(function*() {
        yield* parseStatements(node, context);
    });
    yield "}";

    function* getFirstLineTrailingComments(): PrintItemIterator {
        if (!node.trailingComments)
            return;

        for (const trailingComment of node.trailingComments) {
            if (trailingComment.loc!.start.line === node.loc!.start.line) {
                if (trailingComment.type === "CommentLine")
                    yield " ";
                yield* parseComment(trailingComment, context);
            }
        }
    }
}

function parseIdentifier(node: babel.Identifier) {
    return node.name;
}

/* declarations */

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): PrintItemIterator {
    yield "import ";
    const { specifiers } = node;
    const defaultImport = specifiers.find(s => s.type === "ImportDefaultSpecifier");
    const namespaceImport = specifiers.find(s => s.type === "ImportNamespaceSpecifier");
    const namedImports = specifiers.filter(s => s.type === "ImportSpecifier");

    if (defaultImport) {
        yield parseNode(defaultImport, context);
        if (namespaceImport != null || namedImports.length > 0)
            yield ", "
    }
    if (namespaceImport)
        yield parseNode(namespaceImport, context);

    yield* parseNamedImports();

    if (defaultImport != null || namespaceImport != null || namedImports.length > 0)
        yield " from ";

    yield parseNode(node.source, context);

    if (context.options.semiColons)
        yield ";";

    function* parseNamedImports(): PrintItemIterator {
        if (namedImports.length === 0)
            return;

        const useNewLines = getUseNewLines();
        const braceSeparator = useNewLines ? context.options.newLineKind : " ";

        yield "{";
        yield braceSeparator;

        if (useNewLines)
            yield* withIndent(parseSpecifiers);
        else
            yield* withHangingIndent(parseSpecifiers);

        yield braceSeparator;
        yield "}";

        function getUseNewLines() {
            if (namedImports.length === 1 && namedImports[0].loc!.start.line !== node.loc!.start.line)
                return true;
            return getUseNewLinesForNodes(namedImports);
        }

        function* parseSpecifiers(): PrintItemIterator {
            for (let i = 0; i < namedImports.length; i++) {
                if (i > 0) {
                    yield ",";
                    yield useNewLines ? context.options.newLineKind : Behaviour.SpaceOrNewLine;
                }
                yield parseNode(namedImports[i], context);
            }
        }
    }
}

function parseImportDefaultSpecifier(specifier: babel.ImportDefaultSpecifier, context: Context) {
    return parseNode(specifier.local, context);
}

function* parseImportNamespaceSpecifier(specifier: babel.ImportNamespaceSpecifier, context: Context): PrintItemIterator {
    yield "* as ";
    yield parseNode(specifier.local, context);
}

function* parseImportSpecifier(specifier: babel.ImportSpecifier, context: Context): PrintItemIterator {
    if (specifier.imported.start === specifier.local.start) {
        yield parseNode(specifier.imported, context)
        return;
    }

    yield parseNode(specifier.imported, context);
    yield " as ";
    yield parseNode(specifier.local, context);
}

function* parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context): PrintItemIterator {
    yield "export ";
    yield parseNode(node.declaration, context);
}

function* parseFunctionDeclaration(node: babel.FunctionDeclaration, context: Context): PrintItemIterator {
    yield* parseHeader();
    yield parseNode(node.body, context);

    function* parseHeader(): PrintItemIterator {
        const functionHeaderStartInfo = createInfo("functionHeaderStart");
        yield functionHeaderStartInfo;
        if (node.async)
            yield "async ";
        yield "function";
        if (node.generator)
            yield "*";
        if (node.id) {
            yield " ";
            yield parseNode(node.id, context);
        }
        if (node.typeParameters && node.typeParameters.type !== "Noop")
            yield parseTypeParameterDeclaration(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context);

        if (node.returnType && node.returnType.type !== "Noop") {
            yield ": ";
            yield parseNode(node.returnType.typeAnnotation, context);
        }
        yield newLineIfHangingSpaceOtherwise(context, functionHeaderStartInfo);
    }
}

function parseTypeParameterDeclaration(declaration: babel.TypeParameterDeclaration | babel.TSTypeParameterDeclaration, context: Context): Group {
    const useNewLines = getUseNewLinesForNodes(declaration.params);
    return {
        kind: PrintItemKind.Group,
        items: parseItems()
    };

    function* parseItems(): PrintItemIterator {
        yield "<";

        if (useNewLines)
            yield* surroundWithNewLines(withIndent(parseParameterList()), context);
        else
            yield* withHangingIndent(parseParameterList());

        yield ">";
    }

    function* parseParameterList(): PrintItemIterator {
        const params = declaration.params;
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            yield parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                yield Behaviour.SpaceOrNewLine;
            }
        }
    }
}

function* parseTypeAlias(node: babel.TSTypeAliasDeclaration, context: Context): PrintItemIterator {
    yield "type ";
    yield parseNode(node.id, context);
    if (node.typeParameters)
        yield parseTypeParameterDeclaration(node.typeParameters, context);
    yield " = ";
    yield parseNode(node.typeAnnotation, context);

    if (context.options.semiColons)
        yield ";";
}

/* statements */

function* parseExpressionStatement(node: babel.ExpressionStatement, context: Context): PrintItemIterator {
    yield parseNode(node.expression, context);

    if (context.options.semiColons)
        yield ";";
}

function* parseIfStatement(node: babel.IfStatement, context: Context): PrintItemIterator {
    const startHeaderInfo = createInfo("startHeader");
    yield startHeaderInfo;
    yield "if (";
    yield parseNode(node.test, context);
    yield ")";
    const endHeaderInfo = createInfo("endHeader");
    yield endHeaderInfo;

    const requireBraces = consequentRequiresBraces(node.consequent);
    const startStatementsInfo = createInfo("startStatements");
    const endStatementsInfo = createInfo("endStatements");

    yield {
        kind: PrintItemKind.Condition,
        name: "openBrace",
        condition: conditionContext => {
            // writing an open brace might make the condition hang, so assume it should
            // not write the open brace until it's been resolved
            return requireBraces
                || isMultipleLines(startHeaderInfo, endHeaderInfo, conditionContext, false)
                || isMultipleLines(startStatementsInfo, endStatementsInfo, conditionContext, false);
        },
        true: [newLineIfHangingSpaceOtherwise(context, startHeaderInfo), "{"]
    };

    yield context.options.newLineKind;
    yield startStatementsInfo;

    if (node.consequent.type === "BlockStatement")
        yield* withIndent(parseStatements(node.consequent, context));
    else
        yield* withIndent(parseNode(node.consequent, context));

    yield endStatementsInfo;
    yield {
        kind: PrintItemKind.Condition,
        name: "closeBrace",
        condition: conditionContext => {
            return requireBraces
                || isMultipleLines(startHeaderInfo, endHeaderInfo, conditionContext, false)
                || isMultipleLines(startStatementsInfo, endStatementsInfo, conditionContext, false);
        },
        true: [{
            kind: PrintItemKind.Condition,
            name: "closeBraceNewLine",
            condition: conditionContext => {
                return !areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
            },
            true: [context.options.newLineKind]
        }, "}"]
    };

    function consequentRequiresBraces(statement: babel.Statement) {
        if (statement.type === "BlockStatement") {
            if (statement.body.length === 1 && !hasLeadingCommentOnDifferentLine(statement.body[0]))
                return false;
            return true;
        }

        return hasLeadingCommentOnDifferentLine(statement);
    }
}

/* expressions */

function* parseCallExpression(node: babel.CallExpression, context: Context): PrintItemIterator {
    yield parseNode(node.callee, context);
    yield* parseParametersOrArguments(node.arguments, context);
}

/* literals */

function parseStringLiteral(node: babel.StringLiteral, context: Context) {
    if (context.options.singleQuotes)
        return `'${node.value.replace(/'/g, `\\'`)}'`;
    return `"${node.value.replace(/"/g, `\\"`)}"`;
}

function parseBooleanLiteral(node: babel.BooleanLiteral, context: Context) {
    return (node.value) ? "true" : "false";
}

function parseUnknownNode(node: babel.Node, context: Context): Unknown {
    const nodeText = context.fileText.substring(node.start!, node.end!);

    context.log(`Not implemented node type: ${node.type} (${nodeText})`);

    return {
        kind: PrintItemKind.Unknown,
        text: removeStringIndentation(nodeText, {
            indentSizeInSpaces: 2,
            isInStringAtPos: () => false // todo: actually check if in a string...
        })
    };
}

/* types */

function* parseTypeParameter(node: babel.TSTypeParameter, context: Context): PrintItemIterator {
    yield node.name!;

    if (node.constraint) {
        yield " extends ";
        yield parseNode(node.constraint, context);
    }

    if (node.default) {
        yield " = ";
        yield parseNode(node.default, context);
    }
}

function* parseUnionType(node: babel.TSUnionType, context: Context): PrintItemIterator {
    const useNewLines = getUseNewLinesForNodes(node.types);
    yield* withHangingIndent(function*() {
        for (let i = 0; i < node.types.length; i++) {
            if (i > 0) {
                yield useNewLines ? context.options.newLineKind : Behaviour.SpaceOrNewLine;
                yield "| ";
            }
            yield parseNode(node.types[i], context);
        }
    });
}

/* general */

function* parseStatements(block: babel.BlockStatement | babel.Program, context: Context): PrintItemIterator {
    const statements = block.body;
    for (let i = 0; i < statements.length; i++) {
        if (i > 0) {
            if (hasBody(statements[i - 1]) || hasBody(statements[i])) {
                yield context.options.newLineKind;
                yield context.options.newLineKind;
            }
            else {
                // todo: check if there is a blank line between statements and if so, respect that
                yield context.options.newLineKind;
            }
        }

        yield parseNode(statements[i], context);
    }

    if (block.innerComments)
        yield* parseCommentCollection(block.innerComments, undefined, context);
}

function* parseParametersOrArguments(params: babel.Node[], context: Context): PrintItemIterator {
    const useNewLines = useNewLinesForParametersOrArguments(params);
    yield "(";

    if (useNewLines)
        yield* surroundWithNewLines(withIndent(parseParameterList), context);
    else
        yield* withHangingIndent(parseParameterList);

    yield ")";

    function* parseParameterList(): PrintItemIterator {
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            yield parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                yield useNewLines ? context.options.newLineKind : Behaviour.SpaceOrNewLine;
            }
        }
    }
}

/* reusable conditions */

function getIsHangingCondition(info: Info): Condition {
    return {
        kind: PrintItemKind.Condition,
        name: "isHangingCondition",
        condition: conditionContext => {
            const resolvedInfo = conditionContext.getResolvedInfo(info);
            if (resolvedInfo == null)
                return undefined;
            const isHanging = conditionContext.writerInfo.lineStartIndentLevel > resolvedInfo.lineStartIndentLevel;
            return isHanging;
        }
    }
}

function newLineIfHangingSpaceOtherwise(context: Context, info: Info): Condition {
    return {
        kind: PrintItemKind.Condition,
        name: "newLineIfHangingSpaceOtherwise",
        condition: conditionContext => {
            const resolvedInfo = conditionContext.getResolvedInfo(info);
            if (resolvedInfo == null)
                return undefined;
            const isHanging = conditionContext.writerInfo.lineStartIndentLevel > resolvedInfo.lineStartIndentLevel;
            return isHanging;
        },
        true: [context.options.newLineKind],
        false: [" "]
    }
}

/* helpers */

function* getWithComments(node: babel.Node, nodePrintItem: PrintItem | PrintItemIterator, context: Context): PrintItemIterator {
    yield* parseLeadingComments();

    if (isPrintItemIterator(nodePrintItem))
        yield* nodePrintItem;
    else
        yield nodePrintItem;

    yield* parseTrailingComments();

    function* parseLeadingComments() {
        if (!node.leadingComments)
            return;

        yield* parseCommentCollection(node.leadingComments, undefined, context)
    }

    function* parseTrailingComments() {
        if (!node.trailingComments)
            return;

        yield* parseCommentCollection(node.trailingComments, node, context)
    }
}

function* parseCommentCollection(comments: readonly babel.Comment[], lastNode: (babel.Node | babel.Comment | undefined), context: Context) {
    for (const comment of comments) {
        if (context.handledComments.has(comment))
            continue;

        if (lastNode != null) {
            if (lastNode.loc!.end.line < comment.loc.start.line - 1) {
                yield context.options.newLineKind;
                yield context.options.newLineKind;
            }
            if (lastNode.loc!.end.line < comment.loc.start.line)
                yield context.options.newLineKind;
            else if (comment.type === "CommentLine")
                yield " ";
            else if (lastNode.type === "CommentBlock")
                yield " ";
        }

        yield* parseComment(comment, context);
        lastNode = comment;
    }
}

function* parseComment(comment: babel.Comment, context: Context): PrintItemIterator {
    if (context.handledComments.has(comment))
        return;
    else
        context.handledComments.add(comment);

    switch (comment.type) {
        case "CommentBlock":
            yield* parseCommentBlock(comment);
            break;
        case "CommentLine":
            yield* parseCommentLine(comment);
            break;
        default:
            assertNever(comment);
    }

    function* parseCommentBlock(comment: babel.CommentBlock): PrintItemIterator {
        yield "/*";
        yield comment.value;
        yield "*/";
    }

    function* parseCommentLine(comment: babel.CommentLine): PrintItemIterator {
        yield `// ${comment.value.trim()}`;
        yield Behaviour.ExpectNewLine;
    }
}

function useNewLinesForParametersOrArguments(params: babel.Node[]) {
    return getUseNewLinesForNodes(params);
}

function getUseNewLinesForNodes(nodes: babel.Node[]) {
    if (nodes.length <= 1)
        return false;
    if (nodes[0].loc!.start.line === nodes[1].loc!.start.line)
        return false;
    return true;
}

function* surroundWithNewLines(item: Group | PrintItemIterator | (() => PrintItemIterator), context: Context): PrintItemIterator {
    yield context.options.newLineKind;
    if (item instanceof Function)
        yield* item()
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield context.options.newLineKind;
}

function* withIndent(item: Group | PrintItemIterator | (() => PrintItemIterator)): PrintItemIterator {
    yield Behaviour.StartIndent;
    if (item instanceof Function)
        yield* item()
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield Behaviour.FinishIndent;
}

function* withHangingIndent(item: Group | PrintItemIterator | (() => PrintItemIterator)): PrintItemIterator {
    yield Behaviour.StartHangingIndent;
    if (item instanceof Function)
        yield* item()
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield Behaviour.FinishHangingIndent;
}

function isMultipleLines(startInfo: Info, endInfo: Info, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);
    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;
    return resolvedEndInfo.lineNumber > resolvedStartInfo.lineNumber;
}

function areInfoEqual(startInfo: Info, endInfo: Info, conditionContext: ResolveConditionContext, defaultValue: boolean) {
    const resolvedStartInfo = conditionContext.getResolvedInfo(startInfo);
    const resolvedEndInfo = conditionContext.getResolvedInfo(endInfo);

    if (resolvedStartInfo == null || resolvedEndInfo == null)
        return defaultValue;

    return resolvedStartInfo.lineNumber === resolvedEndInfo.lineNumber
        && resolvedStartInfo.columnNumber === resolvedEndInfo.columnNumber;
}

/* checks */

function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

function hasLeadingCommentOnDifferentLine(node: babel.Node) {
    return node.leadingComments != null
        && node.leadingComments.some(c => c.type === "CommentLine" || c.loc!.start.line < node.loc!.start.line);
}

function createInfo(name: string): Info {
    return {
        kind: PrintItemKind.Info,
        name
    };
}
