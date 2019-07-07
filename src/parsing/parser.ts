import * as babel from "@babel/types";
import { ResolvedConfiguration, resolveNewLineKindFromText, Configuration } from "../configuration";
import { PrintItem, PrintItemKind, Group, Behaviour, Unknown, PrintItemIterator, Condition, Info } from "../types";
import { assertNever, isPrintItemIterator, throwError } from "../utils";
import * as conditions from "./conditions";
import * as nodeHelpers from "./nodeHelpers";
import * as infoChecks from "./infoChecks";

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
    IfStatementLastBraceCondition: "ifStatementLastBraceCondition",
    ClassDeclarationStartHeaderInfo: "classDeclarationStartHeaderInfo"
} as const;

export interface Context {
    file: babel.File,
    fileText: string;
    log: (message: string) => void;
    warn: (message: string) => void;
    config: ResolvedConfiguration;
    handledComments: Set<babel.Comment>;
    /** This is used to queue up the next item on the parent stack. */
    currentNode: babel.Node;
    parentStack: babel.Node[];
    parent: babel.Node;
    newLineKind: "\r\n" | "\n";
    bag: Bag;
}

export function parseFile(file: babel.File, fileText: string, options: ResolvedConfiguration): Group {
    const context: Context = {
        file,
        fileText,
        log: message => console.log("[dprint]: " + message), // todo: use environment?
        warn: message => console.warn("[dprint]: " + message),
        config: options,
        handledComments: new Set<babel.Comment>(),
        currentNode: file,
        parentStack: [],
        parent: file,
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
    "ClassDeclaration": parseClassDeclaration,
    "ExportAllDeclaration": parseExportAllDeclaration,
    "ExportNamedDeclaration": parseExportNamedDeclaration,
    "ExportDefaultDeclaration": parseExportDefaultDeclaration,
    "FunctionDeclaration": parseFunctionDeclaration,
    "ImportDeclaration": parseImportDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    /* class */
    "ClassBody": parseClassBody,
    /* statements */
    "Directive": parseDirective,
    "DoWhileStatement": parseDoWhileStatement,
    "ExpressionStatement": parseExpressionStatement,
    "IfStatement": parseIfStatement,
    "InterpreterDirective": parseInterpreterDirective,
    "TryStatement": parseTryStatement,
    "WhileStatement": parseWhileStatement,
    /* clauses */
    "CatchClause": parseCatchClause,
    /* expressions */
    "BinaryExpression": parseBinaryOrLogicalExpression,
    "CallExpression": parseCallExpression,
    "LogicalExpression": parseBinaryOrLogicalExpression,
    "OptionalCallExpression": parseCallExpression,
    "YieldExpression": parseYieldExpression,
    /* imports */
    "ImportDefaultSpecifier": parseImportDefaultSpecifier,
    "ImportNamespaceSpecifier": parseImportNamespaceSpecifier,
    "ImportSpecifier": parseImportSpecifier,
    /* exports */
    "ExportDefaultSpecifier": parseExportDefaultSpecifier,
    "ExportNamespaceSpecifier": parseExportNamespaceSpecifier,
    "ExportSpecifier": parseExportSpecifier,
    /* literals */
    "BigIntLiteral": parseBigIntLiteral,
    "BooleanLiteral": parseBooleanLiteral,
    "DirectiveLiteral": parseStringOrDirectiveLiteral,
    "NullLiteral": () => "null",
    "NullLiteralTypeAnnotaion": () => "null",
    "NumericLiteral": parseNumericLiteral,
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
    context.parent = context.currentNode;
    context.currentNode = node;

    const parseFunc = parseObj[node!.type] || parseUnknownNode;
    const printItem = parseFunc(node, context);
    const group: Group = {
        kind: PrintItemKind.Group,
        items: innerGetWithComments()
    };

    return group;

    function* innerGetWithComments(): PrintItemIterator {
        yield* getWithComments(node!, printItem, context);

        // replace the past item after iterating
        context.currentNode = context.parentStack.pop()!;
        context.parent = context.parentStack[context.parentStack.length - 1];
    }
}

/* file */
function* parseProgram(node: babel.Program, context: Context): PrintItemIterator {
    if (node.interpreter) {
        yield parseNode(node.interpreter, context);
        yield context.newLineKind;

        if (nodeHelpers.hasSeparatingBlankLine(node.interpreter, node.directives[0] || node.body[0]))
            yield context.newLineKind;
    }

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
            return !infoChecks.areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
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

function* parseIdentifier(node: babel.Identifier, context: Context): PrintItemIterator {
    yield node.name;
    if (node.optional)
        yield "?";
    if (node.typeAnnotation)
        yield parseNode(node.typeAnnotation, context);

    if (context.parent.type === "ExportDefaultDeclaration")
        yield ";"; // todo: configuration
}

/* declarations */

function* parseClassDeclaration(node: babel.ClassDeclaration, context: Context): PrintItemIterator {
    yield* parseClassDecorators();
    yield* parseHeader();
    yield parseNode(node.body, context);

    function* parseClassDecorators(): PrintItemIterator {
        if (context.parent.type === "ExportNamedDeclaration" || context.parent.type === "ExportDefaultDeclaration")
            return;

        // it is a class, but reuse this
        yield* parseDecoratorsIfClass(node, context);
    }

    function* parseHeader(): PrintItemIterator {
        const startHeaderInfo = createInfo("startHeader");
        yield startHeaderInfo;

        context.bag.put(BAG_KEYS.ClassDeclarationStartHeaderInfo, startHeaderInfo);

        if (node.declare)
            yield "declare ";
        if (node.abstract)
            yield "abstract ";
        yield "class";

        if (node.id) {
            yield " ";
            yield parseNode(node.id, context);
        }

        if (node.typeParameters)
            yield parseNode(node.typeParameters, context);

        yield* withHangingIndent(parseExtendsAndImplements());

        function* parseExtendsAndImplements(): PrintItemIterator {
            if (node.superClass) {
                const beforeExtendsInfo = createInfo("beforeExtends");
                yield beforeExtendsInfo;

                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo, beforeExtendsInfo);
                yield "extends ";
                yield* withHangingIndent(parseNode(node.superClass, context));
            }

            if (node.implements && node.implements.length > 0) {
                const beforeImplementsInfo = createInfo("beforeImplements");
                yield beforeImplementsInfo;

                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo, beforeImplementsInfo);
                yield "implements ";
                yield* newlineGroup(withHangingIndent(parseImplements()));
            }
        }
    }

    function* parseImplements(): PrintItemIterator {
        if (node.implements == null)
            return;

        for (let i = 0; i < node.implements.length; i++) {
            if (i > 0) {
                yield ",";
                yield Behaviour.SpaceOrNewLine;
            }
            yield parseNode(node.implements[i], context);
        }
    }
}

function* parseExportAllDeclaration(node: babel.ExportAllDeclaration, context: Context): PrintItemIterator {
    yield "export * from ";
    yield parseNode(node.source, context);
    yield ";"; // todo: configuration
}

function* parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context): PrintItemIterator {
    const { specifiers } = node;
    const defaultExport = specifiers.find(s => s.type === "ExportDefaultSpecifier");
    const namespaceExport = specifiers.find(s => s.type === "ExportNamespaceSpecifier");
    const namedExports = specifiers.filter(s => s.type === "ExportSpecifier") as babel.ExportSpecifier[];

    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export ";

    if (node.declaration)
        yield parseNode(node.declaration, context);
    else if (defaultExport)
        yield parseNode(defaultExport, context);
    else if (namedExports.length > 0)
        yield* parseNamedImportsOrExports(node, namedExports, context);
    else if (namespaceExport)
        yield parseNode(namespaceExport, context);
    else
        yield "{}";

    if (node.source) {
        yield " from ";
        yield parseNode(node.source, context);
    }

    if (node.declaration == null)
        yield ";"; // todo: configuration
}

function* parseExportDefaultDeclaration(node: babel.ExportDefaultDeclaration, context: Context): PrintItemIterator {
    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export default ";
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
        if (node.typeParameters)
            yield parseNode(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context);

        if (node.returnType && node.returnType.type !== "Noop") {
            yield ": ";
            yield parseNode(node.returnType.typeAnnotation, context);
        }
        yield conditions.newlineIfHangingSpaceOtherwise(context, functionHeaderStartInfo);
    }
}

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): PrintItemIterator {
    yield "import ";
    const { specifiers } = node;
    const defaultImport = specifiers.find(s => s.type === "ImportDefaultSpecifier");
    const namespaceImport = specifiers.find(s => s.type === "ImportNamespaceSpecifier");
    const namedImports = specifiers.filter(s => s.type === "ImportSpecifier") as babel.ImportSpecifier[];

    if (defaultImport) {
        yield parseNode(defaultImport, context);
        if (namespaceImport != null || namedImports.length > 0)
            yield ", "
    }
    if (namespaceImport)
        yield parseNode(namespaceImport, context);

    yield* parseNamedImportsOrExports(node, namedImports, context);

    if (defaultImport != null || namespaceImport != null || namedImports.length > 0)
        yield " from ";

    yield parseNode(node.source, context);

    if (context.config["importDeclaration.semiColon"])
        yield ";";
}

function parseTypeParameterDeclaration(
    declaration: babel.TypeParameterDeclaration | babel.TSTypeParameterDeclaration | babel.TSTypeParameterInstantiation | babel.TypeParameterInstantiation,
    context: Context
): Group {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes(declaration.params);
    return {
        kind: PrintItemKind.Group,
        items: newlineGroup(parseItems())
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
                if (useNewLines)
                    yield context.newLineKind;
                else
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
    yield* newlineGroup(parseNode(node.typeAnnotation, context));

    if (context.config["typeAlias.semiColon"])
        yield ";";
}

/* class */

function* parseClassBody(node: babel.ClassBody, context: Context): PrintItemIterator {
    const startHeaderInfo = context.bag.take(BAG_KEYS.ClassDeclarationStartHeaderInfo) as Info | undefined;
    yield* parseBraceSeparator(context.config["classDeclaration.bracePosition"], node, startHeaderInfo, context);

    yield "{";
    yield* withIndent(parseBody());
    yield context.newLineKind;
    yield "}";

    function* parseBody(): PrintItemIterator {
        for (let i = 0; i < node.body.length; i++) {
            yield context.newLineKind;
            yield parseNode(node.body[i], context);
        }
    }
}

/* statements */

function* parseDirective(node: babel.Directive, context: Context): PrintItemIterator {
    yield parseNode(node.value, context);
    if (context.config["directive.semiColon"])
        yield ";";
}

function* parseDoWhileStatement(node: babel.DoWhileStatement, context: Context): PrintItemIterator {
    // the braces are technically optional on do while statements...
    yield "do";
    yield* parseBraceSeparator(context.config["doWhileStatement.bracePosition"], node.body, undefined, context);
    yield parseNode(node.body, context);
    yield " while (";
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
        useBraces: context.config["ifStatement.useBraces"],
        bracePosition: context.config["ifStatement.bracePosition"],
        requiresBracesCondition: context.bag.take(BAG_KEYS.IfStatementLastBraceCondition) as Condition | undefined
    });

    yield* result.iterator;

    if (node.alternate) {
        if (node.alternate.type === "IfStatement" && node.alternate.alternate == null)
            context.bag.put(BAG_KEYS.IfStatementLastBraceCondition, result.braceCondition);

        yield* parseControlFlowSeparator(context.config["ifStatement.nextControlFlowPosition"], node.alternate, "else", context);
        yield "else";
        if (node.alternate.type === "IfStatement") {
            yield " ";
            yield parseNode(node.alternate, context);
        }
        else {
            yield* parseConditionalBraceBody({
                bodyNode: node.alternate,
                context,
                useBraces: context.config["ifStatement.useBraces"],
                bracePosition: context.config["ifStatement.bracePosition"],
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

function* parseInterpreterDirective(node: babel.InterpreterDirective, context: Context): PrintItemIterator {
    yield "#!";
    yield node.value;
}

function* parseTryStatement(node: babel.TryStatement, context: Context): PrintItemIterator {
    yield "try";
    yield* parseBraceSeparator(context.config["tryStatement.bracePosition"], node.block, undefined, context);
    yield parseNode(node.block, context);

    if (node.handler != null) {
        yield* parseControlFlowSeparator(context.config["tryStatement.nextControlFlowPosition"], node.handler, "catch", context);
        yield parseNode(node.handler, context);
    }

    if (node.finalizer != null) {
        yield* parseControlFlowSeparator(context.config["tryStatement.nextControlFlowPosition"], node.finalizer, "finally", context);
        yield "finally";
        yield* parseBraceSeparator(context.config["tryStatement.bracePosition"], node.finalizer, undefined, context);
        yield parseNode(node.finalizer, context);
    }
}

function* parseWhileStatement(node: babel.WhileStatement, context: Context): PrintItemIterator {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "while (";
    yield* withHangingIndent(parseNode(node.test, context));
    yield ")";
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        bodyNode: node.body,
        useBraces: context.config["whileStatement.useBraces"],
        bracePosition: context.config["whileStatement.bracePosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;
}

/* clauses */

function* parseCatchClause(node: babel.CatchClause, context: Context): PrintItemIterator {
    // a bit overkill since the param will currently always be just an identifier
    const startHeaderInfo = createInfo("catchClauseHeaderStart");
    const endHeaderInfo = createInfo("catchClauseHeaderEnd");
    yield startHeaderInfo;
    yield "catch";
    if (node.param != null) {
        yield " (";
        yield* withHangingIndent(parseNode(node.param, context));
        yield ")";
    }

    // not conditional... required.
    yield* parseConditionalBraceBody({
        context,
        bodyNode: node.body,
        useBraces: "always",
        requiresBracesCondition: undefined,
        bracePosition: context.config["tryStatement.bracePosition"],
        startHeaderInfo,
        endHeaderInfo
    }).iterator;
}

interface ParseHeaderWithConditionalBraceBodyOptions {
    bodyNode: babel.Statement;
    parseHeader(): PrintItemIterator;
    context: Context;
    requiresBracesCondition: Condition | undefined;
    useBraces: NonNullable<Configuration["useBraces"]>;
    bracePosition: NonNullable<Configuration["bracePosition"]>;
}

interface ParseHeaderWithConditionalBraceBodyResult {
    iterator: PrintItemIterator;
    braceCondition: Condition;
}

function parseHeaderWithConditionalBraceBody(opts: ParseHeaderWithConditionalBraceBodyOptions): ParseHeaderWithConditionalBraceBodyResult {
    const { bodyNode, context, requiresBracesCondition, useBraces, bracePosition } = opts;
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");

    const result = parseConditionalBraceBody({
        bodyNode,
        context,
        requiresBracesCondition,
        useBraces,
        bracePosition,
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
    useBraces: NonNullable<Configuration["useBraces"]>;
    bracePosition: NonNullable<Configuration["bracePosition"]>;
    requiresBracesCondition: Condition | undefined;
    startHeaderInfo?: Info;
    endHeaderInfo?: Info;
}

interface ParseConditionalBraceBodyResult {
    iterator: PrintItemIterator;
    braceCondition: Condition;
}

function parseConditionalBraceBody(opts: ParseConditionalBraceBodyOptions): ParseConditionalBraceBodyResult {
    const { startHeaderInfo, endHeaderInfo, bodyNode, context, requiresBracesCondition, useBraces, bracePosition } = opts;
    const startStatementsInfo = createInfo("startStatements");
    const endStatementsInfo = createInfo("endStatements");
    const headerTrailingComments = Array.from(getHeaderTrailingComments(bodyNode));
    const openBraceCondition: Condition = {
        kind: PrintItemKind.Condition,
        name: "openBrace",
        condition: conditionContext => {
            if (useBraces === "maintain")
                return bodyNode.type === "BlockStatement";
            else if (useBraces === "always")
                return true;
            else if (useBraces === "preferNone") {
                // writing an open brace might make the header hang, so assume it should
                // not write the open brace until it's been resolved
                return bodyRequiresBraces(bodyNode)
                    || startHeaderInfo && endHeaderInfo && infoChecks.isMultipleLines(startHeaderInfo, endHeaderInfo, conditionContext, false)
                    || infoChecks.isMultipleLines(startStatementsInfo, endStatementsInfo, conditionContext, false)
                    || requiresBracesCondition && conditionContext.getResolvedCondition(requiresBracesCondition);
            }
            else {
                return assertNever(useBraces);
            }
        },
        true: function*(): PrintItemIterator {
            yield* parseBraceSeparator(bracePosition, bodyNode, startHeaderInfo, context);
            yield "{";
        }()
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
                    return !infoChecks.areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
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
            if (bodyNode.body.length === 1 && !nodeHelpers.hasLeadingCommentOnDifferentLine(bodyNode.body[0], /* commentsToIgnore */ headerTrailingComments))
                return false;
            return true;
        }

        return nodeHelpers.hasLeadingCommentOnDifferentLine(bodyNode, /* commentsToIgnore */ headerTrailingComments);
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

function* parseBinaryOrLogicalExpression(node: babel.LogicalExpression | babel.BinaryExpression, context: Context): PrintItemIterator {
    const wasLastSame = context.parent.type === node.type;
    if (wasLastSame)
        yield* parseInner();
    else
        yield* newlineGroup(withHangingIndent(parseInner));

    function* parseInner(): PrintItemIterator {
        yield parseNode(node.left, context);
        yield Behaviour.SpaceOrNewLine;
        yield node.operator;
        yield " ";
        yield parseNode(node.right, context);
    }
}

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

function* parseYieldExpression(node: babel.YieldExpression, context: Context): PrintItemIterator {
    yield "yield";
    if (node.delegate)
        yield "*";
    yield " ";
    yield* withHangingIndent(parseNode(node.argument, context));
}

/* imports */

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

/* exports */

function* parseExportDefaultSpecifier(node: babel.ExportDefaultSpecifier, context: Context): PrintItemIterator {
    yield "default ";
    yield parseNode(node.exported, context);
}

function* parseExportNamespaceSpecifier(node: babel.ExportNamespaceSpecifier, context: Context): PrintItemIterator {
    yield "* as ";
    yield parseNode(node.exported, context);
}

function* parseExportSpecifier(specifier: babel.ExportSpecifier, context: Context): PrintItemIterator {
    if (specifier.local.start === specifier.exported.start) {
        yield parseNode(specifier.local, context)
        return;
    }

    yield parseNode(specifier.local, context);
    yield " as ";
    yield parseNode(specifier.exported, context);
}

/* literals */

function parseBigIntLiteral(node: babel.BigIntLiteral, context: Context) {
    return node.value + "n";
}

function parseBooleanLiteral(node: babel.BooleanLiteral, context: Context) {
    return node.value ? "true" : "false";
}

function parseNumericLiteral(node: babel.NumericLiteral, context: Context) {
    return context.fileText.substring(node.start!, node.end!);
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
        text: nodeText
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
    const useNewLines = nodeHelpers.getUseNewlinesForNodes(node.types);
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
        if (lastNode != null) {
            yield context.newLineKind;
            if (nodeHelpers.hasSeparatingBlankLine(lastNode, directive))
                yield context.newLineKind;
        }

        yield parseNode(directive, context);
        lastNode = directive;
    }

    const statements = block.body;
    for (const statement of statements) {
        if (lastNode != null && !nodeHelpers.hasLeadingCommentOnDifferentLine(statement)) {
            if (nodeHelpers.hasBody(lastNode) || nodeHelpers.hasBody(statement)) {
                yield context.newLineKind;
                yield context.newLineKind;
            }
            else {
                yield context.newLineKind;

                if (nodeHelpers.hasSeparatingBlankLine(lastNode, statement))
                    yield context.newLineKind;
            }
        }

        yield parseNode(statement, context);
        lastNode = statement;
    }

    // get the trailing comments on separate lines of the last node
    if (lastNode != null && lastNode.trailingComments != null) {
        // treat these as if they were leading comments, so don't provide the last node
        yield* parseCommentCollection(lastNode.trailingComments, undefined, context);
    }

    if (block.innerComments && block.innerComments.length > 0) {
        if (lastNode != null)
            yield context.newLineKind;

        yield* parseCommentCollection(block.innerComments, undefined, context);
    }
}

function* parseParametersOrArguments(params: babel.Node[], context: Context): PrintItemIterator {
    const useNewLines = nodeHelpers.useNewlinesForParametersOrArguments(params);
    yield* newlineGroup(parseItems());

    function* parseItems(): PrintItemIterator {
        yield "(";

        if (useNewLines)
            yield* surroundWithNewLines(withIndent(parseParameterList), context);
        else
            yield* withHangingIndent(parseParameterList);

        yield ")";
    }

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

function* parseNamedImportsOrExports(
    parentDeclaration: babel.Node,
    namedImportsOrExports: (babel.ImportSpecifier | babel.ExportSpecifier)[],
    context: Context
): PrintItemIterator {
    if (namedImportsOrExports.length === 0)
        return;

    const useNewLines = getUseNewLines();
    const braceSeparator = useNewLines ? context.newLineKind : " ";

    yield "{";
    yield braceSeparator;

    if (useNewLines)
        yield* withIndent(parseSpecifiers);
    else
        yield* newlineGroup(withHangingIndent(parseSpecifiers));

    yield braceSeparator;
    yield "}";

    function getUseNewLines() {
        if (namedImportsOrExports.length === 1 && namedImportsOrExports[0].loc!.start.line !== parentDeclaration.loc!.start.line)
            return true;
        return nodeHelpers.getUseNewlinesForNodes(namedImportsOrExports);
    }

    function* parseSpecifiers(): PrintItemIterator {
        for (let i = 0; i < namedImportsOrExports.length; i++) {
            if (i > 0) {
                yield ",";
                yield useNewLines ? context.newLineKind : Behaviour.SpaceOrNewLine;
            }
            yield parseNode(namedImportsOrExports[i], context);
        }
    }
}

/* helpers */

function* parseDecoratorsIfClass(declaration: babel.Node | undefined | null, context: Context): PrintItemIterator {
    if (declaration == null || declaration.type !== "ClassDeclaration")
        return;

    if (declaration.decorators == null || declaration.decorators.length === 0)
        return;

    yield* parseDecorators(declaration.decorators, context);
    yield context.newLineKind;
}

function* parseDecorators(decorators: babel.Decorator[], context: Context): PrintItemIterator {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes(decorators);

    for (let i = 0; i < decorators.length; i++) {
        if (i > 0) {
            if (useNewlines)
                yield context.newLineKind;
            else
                yield Behaviour.SpaceOrNewLine;
        }

        yield parseNode(decorators[i], context);
    }
}

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
    const lastComment = node.leadingComments[node.leadingComments.length - 1];
    const hasHandled = lastComment == null || context.handledComments.has(lastComment);

    yield* parseCommentCollection(node.leadingComments, undefined, context)

    if (lastComment != null && !hasHandled && node.loc!.start.line > lastComment.loc!.end.line) {
        yield context.newLineKind;

        if (node.loc!.start.line - 1 > lastComment.loc!.end.line)
            yield context.newLineKind;
    }
}

function* parseTrailingComments(node: babel.Node, context: Context) {
    if (!node.trailingComments)
        return;

    // use the roslyn definition of trailing comments
    const trailingCommentsOnSameLine = node.trailingComments.filter(c => c.loc!.start.line === node.loc!.end.line);
    yield* parseCommentCollection(trailingCommentsOnSameLine, node, context)

    const nextComment = node.trailingComments[trailingCommentsOnSameLine.length];
    if (nextComment != null && !context.handledComments.has(nextComment)) {
        yield context.newLineKind;
        if (nextComment.loc!.start.line > node.loc!.end.line + 1)
            yield context.newLineKind;
    }
}

function* parseCommentCollection(comments: Iterable<babel.Comment>, lastNode: (babel.Node | babel.Comment | undefined), context: Context) {
    for (const comment of comments) {
        if (context.handledComments.has(comment))
            continue;

        if (lastNode != null) {
            if (comment.loc.start.line > lastNode.loc!.end.line) {
                yield context.newLineKind;

                if (comment.loc.start.line > lastNode.loc!.end.line + 1)
                    yield context.newLineKind;
            }
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

function* parseBraceSeparator(bracePosition: NonNullable<Configuration["bracePosition"]>, blockNode: babel.Node, startHeaderInfo: Info | undefined, context: Context) {
    if (bracePosition === "nextLineIfHanging") {
        if (startHeaderInfo == null) {
            yield " ";
        }
        else {
            yield conditions.newlineIfHangingSpaceOtherwise(context, startHeaderInfo);
        }
    }
    else if (bracePosition === "currentLine")
        yield " ";
    else if (bracePosition === "nextLine")
        yield context.newLineKind
    else if (bracePosition === "maintain") {
        if (nodeHelpers.isFirstNodeOnLine(blockNode, context))
            yield context.newLineKind;
        else
            yield " ";
    }
    else {
        assertNever(bracePosition);
    }
}

function* parseControlFlowSeparator(
    nextControlFlowPosition: NonNullable<Configuration["nextControlFlowPosition"]>,
    nodeBlock: babel.Node,
    tokenText: string,
    context: Context
): PrintItemIterator {
    if (nextControlFlowPosition === "currentLine")
        yield " ";
    else if (nextControlFlowPosition === "nextLine")
        yield context.newLineKind
    else if (nextControlFlowPosition === "maintain") {
        const token = getFirstControlFlowToken();
        if (token != null && nodeHelpers.isFirstNodeOnLine(token, context))
            yield context.newLineKind;
        else
            yield " ";
    }
    else {
        assertNever(nextControlFlowPosition);
    }

    function getFirstControlFlowToken() {
        // todo: something faster than O(n)
        const nodeBlockStart = nodeBlock.start!;
        return nodeHelpers.getLastToken(context.file, token => {
            if (token.start > nodeBlockStart)
                return false;
            return token.value === tokenText;
        });
    }
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

function* newlineGroup(item: Group | PrintItemIterator | (() => PrintItemIterator)): PrintItemIterator {
    yield Behaviour.StartNewlineGroup;
    if (item instanceof Function)
        yield* item()
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield Behaviour.FinishNewLineGroup;
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

/* factory functions */

function createInfo(name: string): Info {
    return {
        kind: PrintItemKind.Info,
        name
    };
}

