import * as babel from "@babel/types";
import { ResolvedConfiguration, resolveNewLineKindFromText } from "../configuration";
import { PrintItem, PrintItemKind, Group, Behaviour, Unknown, PrintItemIterator, Condition, Info, ResolveConditionContext } from "../types";
import { assertNever, removeStringIndentation, isPrintItemIterator } from "../utils";
import { throwError } from "../../dist/utils";

class Bag {
    private readonly bag = new Map<string, object>();
    put(key: string, value: any) {
        this.bag.set(key, value);
    }

    take(key: string) {
        const value = this.bag.get(key);
        this.bag.delete(key);
        return value;
    }
}

const BAG_KEYS = {
    IfStatementLastBraceCondition: "ifStatementLastBraceCondition"
} as const;

interface Context {
    file: babel.File,
    fileText: string;
    log: (message: string) => void;
    config: ResolvedConfiguration;
    handledComments: Set<babel.Comment>;
    /** This is used to queue up the next item on the parent stack. */
    currentNode: babel.Node;
    parentStack: babel.Node[];
    newLineKind: "\r\n" | "\n";
    bag: Bag;
}

export function parseFile(file: babel.File, fileText: string, options: ResolvedConfiguration): Group {
    const context: Context = {
        file,
        fileText,
        log: message => console.log("[dprint]: " + message),
        config: options,
        handledComments: new Set<babel.Comment>(),
        currentNode: file,
        parentStack: [],
        newLineKind: options.newLineKind === "auto" ? resolveNewLineKindFromText(fileText) : options.newLineKind,
        bag: new Bag()
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
                true: [context.newLineKind]
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
    "DoWhileStatement": parseDoWhileStatement,
    "ExpressionStatement": parseExpressionStatement,
    "IfStatement": parseIfStatement,
    "Directive": parseDirective,
    /* expressions */
    "CallExpression": parseCallExpression,
    "OptionalCallExpression": parseCallExpression,
    /* imports */
    "ImportDefaultSpecifier": parseImportDefaultSpecifier,
    "ImportNamespaceSpecifier": parseImportNamespaceSpecifier,
    "ImportSpecifier": parseImportSpecifier,
    /* literals */
    "BigIntLiteral": parseBigIntLiteral,
    "BooleanLiteral": parseBooleanLiteral,
    "DirectiveLiteral": parseStringOrDirectiveLiteral,
    "NullLiteral": () => "null",
    "NullLiteralTypeAnnotaion": () => "null",
    "StringLiteral": parseStringOrDirectiveLiteral,
    "StringLiteralTypeAnnotation": parseStringOrDirectiveLiteral,
    /* keywords */
    "AnyTypeAnnotation": () => "any",
    "ThisExpression": () => "this",
    "TSAnyKeyword": () => "any",
    "TSBooleanKeyword": () => "boolean",
    "TSNeverKeyword": () => "never",
    "TSNullKeyword": () => "null",
    "TSNumberKeyword": () => "number",
    "TSObjectKeyword": () => "object",
    "TSStringKeyword": () => "string",
    "TSSymbolKeyword": () => "symbol",
    "TSUndefinedKeyword": () => "undefined",
    "TSUnknownKeyword": () => "unknown",
    "TSVoidKeyword": () => "unknown",
    "VoidKeyword": () => "void",
    /* types */
    "TSLiteralType": parseTSLiteralType,
    "TSTypeParameter": parseTypeParameter,
    "TSUnionType": parseUnionType,
    "TSTypeParameterDeclaration": parseTypeParameterDeclaration,
    "TypeParameterDeclaration": parseTypeParameterDeclaration,
    "TSTypeParameterInstantiation": parseTypeParameterDeclaration
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
    const startStatementsInfo = createInfo("startStatementsInfo");
    const endStatementsInfo = createInfo("endStatementsInfo");

    yield "{";
    yield* getFirstLineTrailingComments();
    yield context.newLineKind;
    yield startStatementsInfo;
    yield* withIndent(function*() {
        yield* parseStatements(node, context);
    });
    yield endStatementsInfo;
    yield {
        kind: PrintItemKind.Condition,
        name: "endStatementsNewLine",
        condition: conditionContext => {
            return !areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
        },
        true: [context.newLineKind]
    }
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

    if (context.config["importDeclaration.semiColon"])
        yield ";";

    function* parseNamedImports(): PrintItemIterator {
        if (namedImports.length === 0)
            return;

        const useNewLines = getUseNewLines();
        const braceSeparator = useNewLines ? context.newLineKind : " ";

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
                    yield useNewLines ? context.newLineKind : Behaviour.SpaceOrNewLine;
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
        if (node.declare)
            yield "declare ";
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
            yield parseNode(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context);

        if (node.returnType && node.returnType.type !== "Noop") {
            yield ": ";
            yield parseNode(node.returnType.typeAnnotation, context);
        }
        yield newLineIfHangingSpaceOtherwise(context, functionHeaderStartInfo);
    }
}

function parseTypeParameterDeclaration(
    declaration: babel.TypeParameterDeclaration | babel.TSTypeParameterDeclaration | babel.TSTypeParameterInstantiation | babel.TypeParameterInstantiation,
    context: Context
): Group {
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
    if (node.declare)
        yield "declare ";
    yield "type ";
    yield parseNode(node.id, context);
    if (node.typeParameters)
        yield parseNode(node.typeParameters, context);
    yield " = ";
    yield parseNode(node.typeAnnotation, context);

    if (context.config["typeAlias.semiColon"])
        yield ";";
}

/* statements */

function* parseDoWhileStatement(node: babel.DoWhileStatement, context: Context): PrintItemIterator {
    yield "do ";
    yield parseNode(node.body, context);
    yield " while(";
    yield* withHangingIndent(parseNode(node.test, context));
    yield ")";

    if (context.config["doWhileStatement.semiColon"])
        yield ";";
}

function* parseExpressionStatement(node: babel.ExpressionStatement, context: Context): PrintItemIterator {
    yield parseNode(node.expression, context);

    if (context.config["expressionStatement.semiColon"])
        yield ";";
}

function* parseIfStatement(node: babel.IfStatement, context: Context): PrintItemIterator {
    const result = parseHeaderWithConditionalBraceBody({
        parseHeader: () => parseHeader(node),
        bodyNode: node.consequent,
        context,
        forceBraces: context.config["ifStatement.forceBraces"],
        requiresBracesCondition: context.bag.take(BAG_KEYS.IfStatementLastBraceCondition) as Condition | undefined
    });

    yield* result.iterator;

    if (node.alternate) {
        if (node.alternate.type === "IfStatement" && node.alternate.alternate == null)
            context.bag.put(BAG_KEYS.IfStatementLastBraceCondition, result.braceCondition);

        yield context.newLineKind;
        yield "else";
        if (node.alternate.type === "IfStatement") {
            yield " ";
            yield parseNode(node.alternate, context);
        }
        else {
            yield* parseConditionalBraceBody({
                bodyNode: node.alternate,
                context,
                forceBraces: context.config["ifStatement.forceBraces"],
                requiresBracesCondition: result.braceCondition
            }).iterator;
        }
    }

    function* parseHeader(ifStatement: babel.IfStatement): PrintItemIterator {
        yield "if (";
        yield parseNode(ifStatement.test, context);
        yield ")";
    }
}

function* parseDirective(node: babel.Directive, context: Context): PrintItemIterator {
    yield parseNode(node.value, context);
    if (context.config["directive.semiColon"])
        yield ";";
}

interface ParseHeaderWithConditionalBraceBodyOptions {
    bodyNode: babel.Statement;
    parseHeader(): PrintItemIterator;
    context: Context;
    requiresBracesCondition: Condition | undefined;
    forceBraces: boolean;
}

interface ParseHeaderWithConditionalBraceBodyResult {
    iterator: PrintItemIterator;
    braceCondition: Condition;
}

function parseHeaderWithConditionalBraceBody(opts: ParseHeaderWithConditionalBraceBodyOptions): ParseHeaderWithConditionalBraceBodyResult {
    const { bodyNode, context, requiresBracesCondition, forceBraces } = opts;
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");

    const result = parseConditionalBraceBody({
        bodyNode,
        context,
        requiresBracesCondition,
        forceBraces,
        startHeaderInfo,
        endHeaderInfo
    });

    return {
        iterator: function*() {
            yield* parseHeader();
            yield* result.iterator;
        }(),
        braceCondition: result.braceCondition
    }

    function* parseHeader(): PrintItemIterator {
        yield startHeaderInfo;
        yield* opts.parseHeader();
        yield endHeaderInfo;
    }
}

interface ParseConditionalBraceBodyOptions {
    bodyNode: babel.Statement;
    context: Context;
    forceBraces: boolean;
    requiresBracesCondition: Condition | undefined;
    startHeaderInfo?: Info;
    endHeaderInfo?: Info;
}

interface ParseConditionalBraceBodyResult {
    iterator: PrintItemIterator;
    braceCondition: Condition;
}

function parseConditionalBraceBody(opts: ParseConditionalBraceBodyOptions): ParseConditionalBraceBodyResult {
    const { startHeaderInfo, endHeaderInfo, bodyNode, context, requiresBracesCondition } = opts;
    const startStatementsInfo = createInfo("startStatements");
    const endStatementsInfo = createInfo("endStatements");
    const headerTrailingComments = Array.from(getHeaderTrailingComments(bodyNode));
    const requireBraces = opts.forceBraces || bodyRequiresBraces(bodyNode);
    const openBraceCondition: Condition = {
        kind: PrintItemKind.Condition,
        name: "openBrace",
        condition: conditionContext => {
            // writing an open brace might make the header hang, so assume it should
            // not write the open brace until it's been resolved
            return requireBraces
                || startHeaderInfo && endHeaderInfo && isMultipleLines(startHeaderInfo, endHeaderInfo, conditionContext, false)
                || isMultipleLines(startStatementsInfo, endStatementsInfo, conditionContext, false)
                || requiresBracesCondition && conditionContext.getResolvedCondition(requiresBracesCondition);
        },
        true: [startHeaderInfo ? newLineIfHangingSpaceOtherwise(context, startHeaderInfo) : " ", "{"]
    };

    return {
        braceCondition: openBraceCondition,
        iterator: parseBody()
    }

    function* parseBody(): PrintItemIterator {
        yield openBraceCondition;

        yield* parseHeaderTrailingComment();

        yield context.newLineKind;
        yield startStatementsInfo;

        if (bodyNode.type === "BlockStatement") {
            yield* withIndent(function*() {
                // parse the remaining trailing comments inside because some of them are parsed already
                // by parsing the header trailing comments
                yield* parseLeadingComments(bodyNode, context);
                yield* parseStatements(bodyNode as babel.BlockStatement, context);
            }());
            yield* parseTrailingComments(bodyNode, context);
        }
        else
            yield* withIndent(parseNode(bodyNode, context));

        yield endStatementsInfo;
        yield {
            kind: PrintItemKind.Condition,
            name: "closeBrace",
            condition: openBraceCondition,
            true: [{
                kind: PrintItemKind.Condition,
                name: "closeBraceNewLine",
                condition: conditionContext => {
                    return !areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
                },
                true: [context.newLineKind]
            }, "}"]
        };

        function* parseHeaderTrailingComment(): PrintItemIterator {
            const result = parseCommentCollection(headerTrailingComments, undefined, context);
            yield* prependToIterableIfHasItems(result, " "); // add a space
        }
    }

    function bodyRequiresBraces(bodyNode: babel.Statement) {
        if (bodyNode.type === "BlockStatement") {
            if (bodyNode.body.length === 1 && !hasLeadingCommentOnDifferentLine(bodyNode.body[0], /* commentsToIgnore */ headerTrailingComments))
                return false;
            return true;
        }

        return hasLeadingCommentOnDifferentLine(bodyNode, /* commentsToIgnore */ headerTrailingComments);
    }

    function* getHeaderTrailingComments(bodyNode: babel.Node) {
        if (bodyNode.type === "BlockStatement") {
            if (bodyNode.leadingComments != null) {
                const commentLine = bodyNode.leadingComments.find(c => c.type === "CommentLine");
                if (commentLine) {
                    yield commentLine;
                    return;
                }
            }

            if (bodyNode.body.length > 0)
                yield* checkLeadingComments(bodyNode.body[0]);
            else if (bodyNode.innerComments)
                yield* checkComments(bodyNode.innerComments);
        }
        else {
            yield* checkLeadingComments(bodyNode);
        }

        function* checkLeadingComments(node: babel.Node) {
            const leadingComments = node.leadingComments;
            if (leadingComments)
                yield* checkComments(leadingComments);
        }

        function* checkComments(comments: ReadonlyArray<babel.Comment>) {
            for (const comment of comments) {
                if (comment.loc.start.line === bodyNode.loc!.start.line)
                    yield comment;
            }
        }
    }
}

/* expressions */

function* parseCallExpression(node: babel.CallExpression | babel.OptionalCallExpression, context: Context): PrintItemIterator {
    yield parseNode(node.callee, context);

    // todo: why does this have both arguments and parameters? Seems like only type parameters are filled
    // I'm guessing typeParameters are used for TypeScript and typeArguments are used for flow?
    if (node.typeArguments != null)
        throwError("Unimplemented scenario where a call expression had type arguments.");

    if (node.typeParameters)
        yield parseNode(node.typeParameters, context);

    if (node.optional)
        yield "?.";

    yield* parseParametersOrArguments(node.arguments, context);
}

/* literals */

function parseBigIntLiteral(node: babel.BigIntLiteral, context: Context) {
    return node.value + "n";
}

function parseBooleanLiteral(node: babel.BooleanLiteral, context: Context) {
    return node.value ? "true" : "false";
}

function parseStringOrDirectiveLiteral(node: babel.StringLiteral | babel.StringLiteralTypeAnnotation | babel.DirectiveLiteral, context: Context) {
    if (context.config.singleQuotes)
        return `'${node.value.replace(/'/g, `\\'`)}'`;
    return `"${node.value.replace(/"/g, `\\"`)}"`;
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

function* parseTSLiteralType(node: babel.TSLiteralType, context: Context): PrintItemIterator {
    yield parseNode(node.literal, context);
}

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
                yield useNewLines ? context.newLineKind : Behaviour.SpaceOrNewLine;
                yield "| ";
            }
            yield parseNode(node.types[i], context);
        }
    });
}

/* general */

function* parseStatements(block: babel.BlockStatement | babel.Program, context: Context): PrintItemIterator {
    let lastNode: babel.Node | undefined;
    for (const directive of block.directives) {
        if (lastNode != null)
            yield context.newLineKind;

        yield parseNode(directive, context);
        lastNode = directive;
    }

    const statements = block.body;
    for (const statement of statements) {
        if (lastNode != null) {
            if (hasBody(lastNode) || hasBody(statement)) {
                yield context.newLineKind;
                yield context.newLineKind;
            }
            else {
                // todo: check if there is a blank line between statements and if so, respect that
                yield context.newLineKind;
            }
        }

        yield parseNode(statement, context);
        lastNode = statement;
    }

    if (block.innerComments && block.innerComments.length > 0) {
        if (lastNode != null)
            yield context.newLineKind;

        yield* parseCommentCollection(block.innerComments, undefined, context);
    }
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
                yield useNewLines ? context.newLineKind : Behaviour.SpaceOrNewLine;
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
        true: [context.newLineKind],
        false: [" "]
    }
}

/* helpers */

function* getWithComments(node: babel.Node, nodePrintItem: PrintItem | PrintItemIterator, context: Context): PrintItemIterator {
    yield* parseLeadingComments(node, context);

    if (isPrintItemIterator(nodePrintItem))
        yield* nodePrintItem;
    else
        yield nodePrintItem;

    yield* parseTrailingComments(node, context);
}

function* parseLeadingComments(node: babel.Node, context: Context) {
    if (!node.leadingComments)
        return;

    yield* parseCommentCollection(node.leadingComments, undefined, context)
}

function* parseTrailingComments(node: babel.Node, context: Context) {
    if (!node.trailingComments)
        return;

    yield* parseCommentCollection(node.trailingComments, node, context)
}

function* parseCommentCollection(comments: Iterable<babel.Comment>, lastNode: (babel.Node | babel.Comment | undefined), context: Context) {
    for (const comment of comments) {
        if (context.handledComments.has(comment))
            continue;

        if (lastNode != null) {
            if (lastNode.loc!.end.line < comment.loc.start.line - 1) {
                yield context.newLineKind;
                yield context.newLineKind;
            }
            if (lastNode.loc!.end.line < comment.loc.start.line)
                yield context.newLineKind;
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
    yield context.newLineKind;
    if (item instanceof Function)
        yield* item()
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield context.newLineKind;
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

function* prependToIterableIfHasItems<T>(iterable: Iterable<T>, ...items: T[]) {
    let found = false;
    for (const item of iterable) {
        if (!found) {
            yield* items;
            found = true;
        }
        yield item;
    }
}

/* checks */

function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

function hasLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    return node.leadingComments != null
        && node.leadingComments.some(c => {
            if (commentsToIgnore != null && commentsToIgnore.includes(c))
                return false;

            return c.type === "CommentLine" || c.loc!.start.line < node.loc!.start.line;
        });
}

function createInfo(name: string): Info {
    return {
        kind: PrintItemKind.Info,
        name
    };
}

