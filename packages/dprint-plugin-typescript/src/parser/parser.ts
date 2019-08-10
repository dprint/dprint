import * as babel from "@babel/types";
import { makeIterableRepeatable, PrintItemKind, Signal, RawString, PrintItemIterable, Condition, Info, parserHelpers, conditions, conditionResolvers,
    resolveNewLineKindFromText } from "@dprint/core";
import { ResolvedTypeScriptConfiguration, TypeScriptConfiguration } from "../configuration/Configuration";
import { assertNever, Bag, Stack } from "../utils";
import * as nodeHelpers from "./nodeHelpers";
import * as tokenHelpers from "./tokenHelpers";
import { TokenFinder } from "./utils";

const { withIndent, newlineGroup, prependToIterableIfHasItems, toPrintItemIterable, surroundWithNewLines } = parserHelpers;

const BAG_KEYS = {
    IfStatementLastBraceCondition: "ifStatementLastBraceCondition",
    ClassStartHeaderInfo: "classStartHeaderInfo",
    InterfaceDeclarationStartHeaderInfo: "interfaceDeclarationStartHeaderInfo",
    ModuleDeclarationStartHeaderInfo: "moduleDeclarationStartHeaderInfo",
    DisableIndentBool: "disableIndentBool"
} as const;

interface Context {
    file: babel.File;
    fileText: string;
    log: (message: string) => void;
    warn: (message: string) => void;
    config: ResolvedTypeScriptConfiguration;
    handledComments: Set<babel.Comment>;
    /** This is used to queue up the next item on the parent stack. */
    currentNode: babel.Node;
    parentStack: babel.Node[];
    parent: babel.Node;
    newlineKind: "\r\n" | "\n";
    bag: Bag;
    endStatementOrMemberInfo: Stack<Info>;
    tokenFinder: TokenFinder;
}

export function parseTypeScriptFile(file: babel.File, fileText: string, options: ResolvedTypeScriptConfiguration): PrintItemIterable | false {
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
        newlineKind: options.newlineKind === "auto" ? resolveNewLineKindFromText(fileText) : options.newlineKind,
        bag: new Bag(),
        endStatementOrMemberInfo: new Stack<Info>(),
        tokenFinder: new TokenFinder(file.tokens)
    };

    if (!shouldParseFile())
        return false; // skip parsing

    return function*(): PrintItemIterable {
        yield* parseNode(file.program, context);
        yield {
            kind: PrintItemKind.Condition,
            name: "endOfFileNewLine",
            condition: conditionContext => {
                return conditionContext.writerInfo.columnNumber > 0 || conditionContext.writerInfo.lineNumber > 0;
            },
            true: [context.newlineKind]
        };
    }();

    function shouldParseFile() {
        for (const comment of getCommentsToCheck()) {
            if (comment.value.indexOf("dprint:ignoreFile") >= 0)
                return false;
        }

        return true;

        function* getCommentsToCheck() {
            const program = file.program;
            if (program.innerComments)
                yield* program.innerComments;
            const body = program.body;
            if (body.length > 0 && body[0].leadingComments != null)
                yield* body[0].leadingComments;
        }
    }
}

const parseObj: { [name: string]: (node: any, context: Context) => PrintItemIterable; } = {
    /* file */
    "Program": parseProgram,
    /* common */
    "BlockStatement": parseBlockStatement,
    "Identifier": parseIdentifier,
    /* declarations */
    "ClassDeclaration": parseClassDeclarationOrExpression,
    "ExportAllDeclaration": parseExportAllDeclaration,
    "ExportNamedDeclaration": parseExportNamedDeclaration,
    "ExportDefaultDeclaration": parseExportDefaultDeclaration,
    "FunctionDeclaration": parseFunctionDeclarationOrExpression,
    "TSDeclareFunction": parseFunctionDeclarationOrExpression,
    "TSEnumDeclaration": parseEnumDeclaration,
    "TSEnumMember": parseEnumMember,
    "ImportDeclaration": parseImportDeclaration,
    "TSImportEqualsDeclaration": parseImportEqualsDeclaration,
    "TSInterfaceDeclaration": parseInterfaceDeclaration,
    "TSModuleDeclaration": parseModuleDeclaration,
    "TSNamespaceExportDeclaration": parseNamespaceExportDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    /* class */
    "ClassBody": parseClassBody,
    "ClassMethod": parseClassOrObjectMethod,
    "TSDeclareMethod": parseClassOrObjectMethod,
    "ClassProperty": parseClassProperty,
    "Decorator": parseDecorator,
    "TSParameterProperty": parseParameterProperty,
    /* interface / type element */
    "TSCallSignatureDeclaration": parseCallSignatureDeclaration,
    "TSConstructSignatureDeclaration": parseConstructSignatureDeclaration,
    "TSIndexSignature": parseIndexSignature,
    "TSInterfaceBody": parseInterfaceBody,
    "TSMethodSignature": parseMethodSignature,
    "TSPropertySignature": parsePropertySignature,
    /* module */
    "TSModuleBlock": parseModuleBlock,
    /* statements */
    "BreakStatement": parseBreakStatement,
    "ContinueStatement": parseContinueStatement,
    "DebuggerStatement": parseDebuggerStatement,
    "Directive": parseDirective,
    "DoWhileStatement": parseDoWhileStatement,
    "EmptyStatement": parseEmptyStatement,
    "TSExportAssignment": parseExportAssignment,
    "ExpressionStatement": parseExpressionStatement,
    "ForInStatement": parseForInStatement,
    "ForOfStatement": parseForOfStatement,
    "ForStatement": parseForStatement,
    "IfStatement": parseIfStatement,
    "InterpreterDirective": parseInterpreterDirective,
    "LabeledStatement": parseLabeledStatement,
    "ReturnStatement": parseReturnStatement,
    "SwitchCase": parseSwitchCase,
    "SwitchStatement": parseSwitchStatement,
    "ThrowStatement": parseThrowStatement,
    "TryStatement": parseTryStatement,
    "WhileStatement": parseWhileStatement,
    "VariableDeclaration": parseVariableDeclaration,
    "VariableDeclarator": parseVariableDeclarator,
    /* clauses */
    "CatchClause": parseCatchClause,
    /* expressions */
    "ArrayPattern": parseArrayPattern,
    "ArrayExpression": parseArrayExpression,
    "ArrowFunctionExpression": parseArrowFunctionExpression,
    "TSAsExpression": parseAsExpression,
    "AssignmentExpression": parseAssignmentExpression,
    "AssignmentPattern": parseAssignmentPattern,
    "AwaitExpression": parseAwaitExpression,
    "BinaryExpression": parseBinaryOrLogicalExpression,
    "LogicalExpression": parseBinaryOrLogicalExpression,
    "CallExpression": parseCallExpression,
    "OptionalCallExpression": parseCallExpression,
    "ClassExpression": parseClassDeclarationOrExpression,
    "ConditionalExpression": parseConditionalExpression,
    "TSExpressionWithTypeArguments": parseExpressionWithTypeArguments,
    "TSExternalModuleReference": parseExternalModuleReference,
    "FunctionExpression": parseFunctionDeclarationOrExpression,
    "MemberExpression": parseMemberExpression,
    "MetaProperty": parseMetaProperty,
    "NewExpression": parseNewExpression,
    "TSNonNullExpression": parseNonNullExpression,
    "ObjectExpression": parseObjectExpression,
    "ObjectMethod": parseClassOrObjectMethod,
    "ObjectPattern": parseObjectPattern,
    "ObjectProperty": parseObjectProperty,
    "RestElement": parseRestElement,
    "SpreadElement": parseSpreadElement,
    "TaggedTemplateExpression": parseTaggedTemplateExpression,
    "TSTypeAssertion": parseTypeAssertion,
    "UnaryExpression": parseUnaryExpression,
    "UpdateExpression": parseUpdateExpression,
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
    "NullLiteral": () => toPrintItemIterable("null"),
    "NumericLiteral": parseNumericLiteral,
    "StringLiteral": parseStringOrDirectiveLiteral,
    "RegExpLiteral": parseRegExpLiteral,
    "TemplateElement": parseTemplateElement,
    "TemplateLiteral": parseTemplateLiteral,
    /* keywords */
    "Import": () => toPrintItemIterable("import"),
    "Super": () => toPrintItemIterable("super"),
    "ThisExpression": () => toPrintItemIterable("this"),
    "TSAnyKeyword": () => toPrintItemIterable("any"),
    "TSBooleanKeyword": () => toPrintItemIterable("boolean"),
    "TSNeverKeyword": () => toPrintItemIterable("never"),
    "TSNullKeyword": () => toPrintItemIterable("null"),
    "TSNumberKeyword": () => toPrintItemIterable("number"),
    "TSObjectKeyword": () => toPrintItemIterable("object"),
    "TSStringKeyword": () => toPrintItemIterable("string"),
    "TSSymbolKeyword": () => toPrintItemIterable("symbol"),
    "TSUndefinedKeyword": () => toPrintItemIterable("undefined"),
    "TSUnknownKeyword": () => toPrintItemIterable("unknown"),
    "TSVoidKeyword": () => toPrintItemIterable("void"),
    "VoidKeyword": () => toPrintItemIterable("void"),
    /* types */
    "TSArrayType": parseArrayType,
    "TSConditionalType": parseConditionalType,
    "TSConstructorType": parseConstructorType,
    "TSFunctionType": parseFunctionType,
    "TSImportType": parseImportType,
    "TSIndexedAccessType": parseIndexedAccessType,
    "TSInferType": parseInferType,
    "TSIntersectionType": parseUnionOrIntersectionType,
    "TSLiteralType": parseLiteralType,
    "TSMappedType": parseMappedType,
    "TSOptionalType": parseOptionalType,
    "TSParenthesizedType": parseParenthesizedType,
    "TSQualifiedName": parseQualifiedName,
    "TSRestType": parseRestType,
    "TSThisType": () => "this",
    "TSTupleType": parseTupleType,
    "TSTypeAnnotation": parseTypeAnnotation,
    "TSTypeLiteral": parseTypeLiteral,
    "TSTypeOperator": parseTypeOperator,
    "TSTypeParameter": parseTypeParameter,
    "TSTypeParameterDeclaration": parseTypeParameterDeclaration,
    "TSTypeParameterInstantiation": parseTypeParameterDeclaration,
    "TSTypePredicate": parseTypePredicate,
    "TSTypeQuery": parseTypeQuery,
    "TSTypeReference": parseTypeReference,
    "TSUnionType": parseUnionOrIntersectionType,
    /* explicitly not implemented (most are proposals that haven't made it far enough) */
    "ArgumentPlaceholder": parseUnknownNode,
    "BindExpression": parseUnknownNode,
    "ClassPrivateMethod": parseUnknownNode,
    "ClassPrivateProperty": parseUnknownNode,
    "DoExpression": parseUnknownNode,
    "Noop": parseUnknownNode,
    "OptionalMemberExpression": parseUnknownNode,
    "ParenthesizedExpression": parseUnknownNode, // seems this is not used?
    "PrivateName": parseUnknownNode,
    "PipelineBareFunction": parseUnknownNode,
    "PipelineTopicExpression": parseUnknownNode,
    "PipelinePrimaryTopicReference": parseUnknownNode,
    "Placeholder": parseUnknownNode,
    "SequenceExpression": parseUnknownNode,
    "WithStatement": parseUnknownNode, // not supported
    /* flow */
    "AnyTypeAnnotation": parseNotSupportedFlowNode,
    "ArrayTypeAnnotation": parseNotSupportedFlowNode,
    "BooleanLiteralTypeAnnotation": parseNotSupportedFlowNode,
    "BooleanTypeAnnotation": parseNotSupportedFlowNode,
    "ClassImplements": parseNotSupportedFlowNode,
    "DeclareClass": parseNotSupportedFlowNode,
    "DeclareExportAllDeclaration": parseNotSupportedFlowNode,
    "DeclareExportDeclaration": parseNotSupportedFlowNode,
    "DeclareFunction": parseNotSupportedFlowNode,
    "DeclareInterface": parseNotSupportedFlowNode,
    "DeclareModule": parseNotSupportedFlowNode,
    "DeclareModuleExports": parseNotSupportedFlowNode,
    "DeclareOpaqueType": parseNotSupportedFlowNode,
    "DeclareTypeAlias": parseNotSupportedFlowNode,
    "DeclareVariable": parseNotSupportedFlowNode,
    "DeclaredPredicate": parseNotSupportedFlowNode,
    "EmptyTypeAnnotation": parseNotSupportedFlowNode,
    "ExistsTypeAnnotation": parseNotSupportedFlowNode,
    "FunctionTypeAnnotation": parseNotSupportedFlowNode,
    "FunctionTypeParam": parseNotSupportedFlowNode,
    "GenericTypeAnnotation": parseNotSupportedFlowNode,
    "InferredPredicate": parseNotSupportedFlowNode,
    "InterfaceDeclaration": parseNotSupportedFlowNode,
    "InterfaceExtends": parseNotSupportedFlowNode,
    "InterfaceTypeAnnotation": parseNotSupportedFlowNode,
    "IntersectionTypeAnnotation": parseNotSupportedFlowNode,
    "MixedTypeAnnotation": parseNotSupportedFlowNode,
    "NullLiteralTypeAnnotation": parseNotSupportedFlowNode,
    "NullableTypeAnnotation": parseNotSupportedFlowNode,
    "NumberLiteralTypeAnnotation": parseNotSupportedFlowNode,
    "NumberTypeAnnotation": parseNotSupportedFlowNode,
    "ObjectTypeAnnotation": parseNotSupportedFlowNode,
    "ObjectTypeCallProperty": parseNotSupportedFlowNode,
    "ObjectTypeIndexer": parseNotSupportedFlowNode,
    "ObjectTypeInternalSlot": parseNotSupportedFlowNode,
    "ObjectTypeProperty": parseNotSupportedFlowNode,
    "ObjectTypeSpreadProperty": parseNotSupportedFlowNode,
    "OpaqueType": parseNotSupportedFlowNode,
    "QualifiedTypeIdentifier": parseNotSupportedFlowNode,
    "StringLiteralTypeAnnotation": parseNotSupportedFlowNode,
    "StringTypeAnnotation": parseNotSupportedFlowNode,
    "ThisTypeAnnotation": parseNotSupportedFlowNode,
    "TupleTypeAnnotation": parseNotSupportedFlowNode,
    "TypeAlias": parseNotSupportedFlowNode,
    "TypeAnnotation": parseNotSupportedFlowNode,
    "TypeCastExpression": parseNotSupportedFlowNode,
    "TypeParameter": parseNotSupportedFlowNode,
    "TypeParameterDeclaration": parseNotSupportedFlowNode,
    "TypeParameterInstantiation": parseNotSupportedFlowNode,
    "TypeofTypeAnnotation": parseNotSupportedFlowNode,
    "UnionTypeAnnotation": parseNotSupportedFlowNode,
    "Variance": parseNotSupportedFlowNode,
    "VoidTypeAnnotation": parseNotSupportedFlowNode
};

interface ParseNodeOptions {
    /**
     * Inner parse useful for adding items at the beginning or end of the iterator
     * after leading comments and before trailing comments.
     */
    innerParse?(iterator: PrintItemIterable): PrintItemIterable;
}

function* parseNode(node: babel.Node | null, context: Context, opts?: ParseNodeOptions): PrintItemIterable {
    if (node == null)
        return;

    // store info
    context.parentStack.push(context.currentNode);
    context.parent = context.currentNode;
    context.currentNode = node;

    // parse
    const parseFunc = parseObj[node!.type] || parseUnknownNode;
    const printItemIterator = opts && opts.innerParse ? opts.innerParse(parseNode()) : parseNode();

    yield* getWithComments(node, printItemIterator, context);

    // replace the past info after iterating
    context.currentNode = context.parentStack.pop()!;
    context.parent = context.parentStack[context.parentStack.length - 1];

    function parseNode() {
        const nodeIterator = parseFunc(node, context);
        return nodeHelpers.hasParentheses(node!) ? parseInParens(nodeIterator) : nodeIterator;
    }

    function parseInParens(nodeIterator: PrintItemIterable) {
        const openParenToken = tokenHelpers.getFirstOpenParenTokenBefore(node!, context)!;
        const useNewLines = nodeHelpers.getUseNewlinesForNodes([openParenToken, node]);

        if (useNewLines)
            putDisableIndentInBagIfNecessaryForNode(node!, context);

        return conditions.withIndentIfStartOfLineIndented(parseIteratorInParens(nodeIterator, useNewLines, context));
    }
}

/* file */
function* parseProgram(node: babel.Program, context: Context): PrintItemIterable {
    if (node.interpreter) {
        yield* parseNode(node.interpreter, context);
        yield context.newlineKind;

        if (nodeHelpers.hasSeparatingBlankLine(node.interpreter, node.directives[0] || node.body[0]))
            yield context.newlineKind;
    }

    yield* parseStatements(node, context);
}

/* common */

function* parseBlockStatement(node: babel.BlockStatement, context: Context): PrintItemIterable {
    const startStatementsInfo = createInfo("startStatementsInfo");
    const endStatementsInfo = createInfo("endStatementsInfo");

    yield "{";

    // Allow: const t = () => {};
    if (context.parent.type === "ArrowFunctionExpression" && node.loc!.start.line === node.loc!.end.line
        && node.body.length === 0 && !node.leadingComments && !node.innerComments)
    {
        yield "}";
        return;
    }

    yield* parseFirstLineTrailingComments(node, node.body, context);
    yield context.newlineKind;
    yield startStatementsInfo;
    yield* withIndent(parseStatements(node, context));
    yield endStatementsInfo;
    yield {
        kind: PrintItemKind.Condition,
        name: "endStatementsNewLine",
        condition: conditionContext => {
            return !conditionResolvers.areInfoEqual(conditionContext, startStatementsInfo, endStatementsInfo, false);
        },
        true: [context.newlineKind]
    };
    yield "}";
}

function* parseIdentifier(node: babel.Identifier, context: Context): PrintItemIterable {
    const parent = context.parent;

    yield node.name;

    if (node.optional)
        yield "?";
    if (parent.type === "VariableDeclarator" && parent.definite)
        yield "!";

    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);

    if (parent.type === "ExportDefaultDeclaration")
        yield ";"; // todo: configuration
}

/* declarations */

function* parseClassDeclarationOrExpression(node: babel.ClassDeclaration | babel.ClassExpression, context: Context): PrintItemIterable {
    if (node.type === "ClassExpression") {
        yield* parseClassDecorators();
        yield {
            kind: PrintItemKind.Condition,
            name: "singleIndentIfStartOfLine",
            condition: conditionResolvers.isStartOfNewLine,
            true: [Signal.SingleIndent]
        };
    }
    else {
        yield* parseClassDecorators();
    }

    yield* parseHeader();

    yield* parseNode(node.body, context);

    function* parseClassDecorators(): PrintItemIterable {
        if (context.parent.type === "ExportNamedDeclaration" || context.parent.type === "ExportDefaultDeclaration")
            return;

        // it is a class, but reuse this
        yield* parseDecoratorsIfClass(node, context);
    }

    function* parseHeader(): PrintItemIterable {
        const startHeaderInfo = createInfo("startHeader");
        yield startHeaderInfo;

        context.bag.put(BAG_KEYS.ClassStartHeaderInfo, startHeaderInfo);

        if (node.type === "ClassDeclaration") {
            if (node.declare)
                yield "declare ";
            if (node.abstract)
                yield "abstract ";
        }

        yield "class";

        if (node.id) {
            yield " ";
            yield* parseNode(node.id, context);
        }

        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        yield* parseExtendsAndImplements();

        function* parseExtendsAndImplements(): PrintItemIterable {
            if (node.superClass) {
                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo);
                yield* conditions.indentIfStartOfLine(function*() {
                    yield "extends ";
                    yield* parseNode(node.superClass, context);
                    if (node.superTypeParameters)
                        yield* parseNode(node.superTypeParameters, context);
                }());
            }

            yield* parseExtendsOrImplements({
                text: "implements",
                items: node.implements,
                context,
                startHeaderInfo
            });
        }
    }
}

function* parseEnumDeclaration(node: babel.TSEnumDeclaration, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    yield* parseHeader();
    yield* parseBody();

    function* parseHeader(): PrintItemIterable {
        yield startHeaderInfo;

        if (node.declare)
            yield "declare ";
        if (node.const)
            yield "const ";
        yield "enum";

        yield " ";
        yield* parseNode(node.id, context);
    }

    function parseBody(): PrintItemIterable {
        return parseMemberedBody({
            bracePosition: context.config["enumDeclaration.bracePosition"],
            context,
            node,
            members: node.members,
            startHeaderInfo,
            shouldUseBlankLine,
            trailingCommas: context.config["enumDeclaration.trailingCommas"]
        });
    }

    function shouldUseBlankLine(previousNode: babel.Node, nextNode: babel.Node) {
        const memberSpacingOption = context.config["enumDeclaration.memberSpacing"];
        switch (memberSpacingOption) {
            case "blankline":
                return true;
            case "newline":
                return false;
            case "maintain":
                return nodeHelpers.hasSeparatingBlankLine(previousNode, nextNode);
            default:
                return assertNever(memberSpacingOption);
        }
    }
}

function* parseEnumMember(node: babel.TSEnumMember, context: Context): PrintItemIterable {
    yield* parseNode(node.id, context);

    if (node.initializer)
        yield* parseInitializer(node.initializer);

    function* parseInitializer(initializer: NonNullable<babel.TSEnumMember["initializer"]>): PrintItemIterable {
        if (initializer.type === "NumericLiteral" || initializer.type === "StringLiteral")
            yield Signal.SpaceOrNewLine;
        else
            yield " ";

        yield* conditions.indentIfStartOfLine(function*() {
            yield "= ";
            yield* parseNode(initializer, context);
        }());
    }
}

function* parseExportAllDeclaration(node: babel.ExportAllDeclaration, context: Context): PrintItemIterable {
    yield "export * from ";
    yield* parseNode(node.source, context);
    yield ";"; // todo: configuration
}

function* parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context): PrintItemIterable {
    const { specifiers } = node;
    const defaultExport = specifiers.find(s => s.type === "ExportDefaultSpecifier");
    const namespaceExport = specifiers.find(s => s.type === "ExportNamespaceSpecifier");
    const namedExports = specifiers.filter(s => s.type === "ExportSpecifier") as babel.ExportSpecifier[];

    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export ";

    if (node.declaration)
        yield* parseNode(node.declaration, context);
    else if (defaultExport)
        yield* parseNode(defaultExport, context);
    else if (namedExports.length > 0)
        yield* parseNamedImportsOrExports(node, namedExports, context);
    else if (namespaceExport)
        yield* parseNode(namespaceExport, context);
    else
        yield "{}";

    if (node.source) {
        yield " from ";
        yield* parseNode(node.source, context);
    }

    if (node.declaration == null)
        yield ";"; // todo: configuration
}

function* parseExportDefaultDeclaration(node: babel.ExportDefaultDeclaration, context: Context): PrintItemIterable {
    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export default ";
    yield* parseNode(node.declaration, context);
}

function* parseFunctionDeclarationOrExpression(
    node: babel.FunctionDeclaration | babel.TSDeclareFunction | babel.FunctionExpression,
    context: Context
): PrintItemIterable {
    yield* parseHeader();
    if (node.type === "FunctionDeclaration" || node.type === "FunctionExpression")
        yield* parseNode(node.body, context);
    else if (context.config["functionDeclaration.semiColon"])
        yield ";";

    function* parseHeader(): PrintItemIterable {
        const startHeaderInfo = createInfo("functionHeaderStart");
        yield startHeaderInfo;
        if (node.type !== "FunctionExpression" && node.declare)
            yield "declare ";
        if (node.async)
            yield "async ";
        yield "function";
        if (node.generator)
            yield "*";
        if (node.id) {
            yield " ";
            yield* parseNode(node.id, context);
        }
        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context, {
            customCloseParen: parseCloseParenWithType({
                context,
                startInfo: startHeaderInfo,
                typeNode: node.returnType
            })
        });

        if (node.type === "FunctionDeclaration" || node.type === "FunctionExpression") {
            const bracePosition = node.type === "FunctionDeclaration"
                ? context.config["functionDeclaration.bracePosition"]
                : context.config["functionExpression.bracePosition"];

            yield* parseBraceSeparator({
                bracePosition,
                bodyNode: node.body,
                startHeaderInfo: startHeaderInfo,
                context
            });
        }
    }
}

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): PrintItemIterable {
    yield "import ";
    const { specifiers } = node;
    const defaultImport = specifiers.find(s => s.type === "ImportDefaultSpecifier");
    const namespaceImport = specifiers.find(s => s.type === "ImportNamespaceSpecifier");
    const namedImports = specifiers.filter(s => s.type === "ImportSpecifier") as babel.ImportSpecifier[];

    if (defaultImport) {
        yield* parseNode(defaultImport, context);
        if (namespaceImport != null || namedImports.length > 0)
            yield ", ";
    }
    if (namespaceImport)
        yield* parseNode(namespaceImport, context);

    yield* parseNamedImportsOrExports(node, namedImports, context);

    if (defaultImport != null || namespaceImport != null || namedImports.length > 0)
        yield " from ";

    yield* parseNode(node.source, context);

    if (context.config["importDeclaration.semiColon"])
        yield ";";
}

function* parseImportEqualsDeclaration(node: babel.TSImportEqualsDeclaration, context: Context): PrintItemIterable {
    if (node.isExport)
        yield "export ";

    yield "import ";
    yield* parseNode(node.id, context);
    yield " = ";
    yield* parseNode(node.moduleReference, context);

    if (context.config["importEqualsDeclaration.semiColon"])
        yield ";";
}

function* parseInterfaceDeclaration(node: babel.TSInterfaceDeclaration, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    yield startHeaderInfo;

    context.bag.put(BAG_KEYS.InterfaceDeclarationStartHeaderInfo, startHeaderInfo);

    if (node.declare)
        yield "declare ";

    yield "interface ";
    yield* parseNode(node.id, context);
    yield* parseNode(node.typeParameters, context);

    yield* parseExtendsOrImplements({
        text: "extends",
        items: node.extends,
        context,
        startHeaderInfo
    });

    yield* parseNode(node.body, context);
}

function* parseModuleDeclaration(node: babel.TSModuleDeclaration, context: Context): PrintItemIterable {
    // doing namespace Name1.Name2 {} is actually two nested module declarations
    if (context.parent.type !== "TSModuleDeclaration") {
        const startHeaderInfo = createInfo("startHeader");
        yield startHeaderInfo;

        context.bag.put(BAG_KEYS.ModuleDeclarationStartHeaderInfo, startHeaderInfo);

        if (node.declare)
            yield "declare ";

        if (node.global) {
            yield "global";
            if (node.id != null)
                yield " ";
        }
        else {
            if (hasNamespaceKeyword())
                yield "namespace ";
            else
                yield "module ";
        }
    }
    else {
        yield ".";
    }

    yield* parseNode(node.id, context);

    if (node.body)
        yield* parseNode(node.body, context);
    else if (context.config["moduleDeclaration.semiColon"])
        yield ";";

    function hasNamespaceKeyword() {
        // todo: something faster
        const keyword = nodeHelpers.getFirstToken(context.file, token => {
            if (token.start < node.start!)
                return false;
            if (token.start > node.end!)
                return "stop";
            if (token.value && (token.value === "namespace" || token.value === "module"))
                return true;
            return false;
        });

        return keyword == null || keyword.value === "namespace";
    }
}

function* parseNamespaceExportDeclaration(node: babel.TSNamespaceExportDeclaration, context: Context): PrintItemIterable {
    yield "export as namespace ";
    yield* parseNode(node.id, context);

    if (context.config["namespaceExportDeclaration.semiColon"])
        yield ";";
}

function* parseTypeAlias(node: babel.TSTypeAliasDeclaration, context: Context): PrintItemIterable {
    if (node.declare)
        yield "declare ";
    yield "type ";
    yield* parseNode(node.id, context);
    if (node.typeParameters)
        yield* parseNode(node.typeParameters, context);
    yield " = ";
    yield* newlineGroup(parseNode(node.typeAnnotation, context));

    if (context.config["typeAlias.semiColon"])
        yield ";";
}

function* parseTypeParameterDeclaration(
    declaration: babel.TSTypeParameterDeclaration | babel.TSTypeParameterInstantiation | babel.TypeParameterInstantiation,
    context: Context
): PrintItemIterable {
    const useNewLines = getUseNewLines();
    yield* newlineGroup(parseItems());

    function* parseItems(): PrintItemIterable {
        yield "<";

        if (useNewLines)
            yield* surroundWithNewLines(parseParameterList(), context);
        else
            yield* parseParameterList();

        yield ">";
    }

    function* parseParameterList(): PrintItemIterable {
        const params = declaration.params;
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            if (i > 0) {
                if (useNewLines)
                    yield context.newlineKind;
                else
                    yield Signal.SpaceOrNewLine;
            }

            yield* conditions.indentIfStartOfLine(parseNode(param, context, {
                innerParse: function*(iterator) {
                    yield* iterator;
                    if (i < params.length - 1)
                        yield ",";
                }
            }));
        }
    }

    function getUseNewLines() {
        if (declaration.params.length === 0)
            return false;

        return nodeHelpers.getUseNewlinesForNodes([
            tokenHelpers.getFirstAngleBracketTokenBefore(declaration.params[0], context),
            declaration.params[0]
        ]);
    }
}

function* parseVariableDeclaration(node: babel.VariableDeclaration, context: Context): PrintItemIterable {
    if (node.declare)
        yield "declare ";
    yield node.kind + " ";

    yield* parseDeclarators();

    if (requiresSemiColon())
        yield ";";

    function* parseDeclarators(): PrintItemIterable {
        for (let i = 0; i < node.declarations.length; i++) {
            if (i > 0) {
                yield ",";
                yield Signal.SpaceOrNewLine;
            }

            yield* conditions.indentIfStartOfLine(parseNode(node.declarations[i], context));
        }
    }

    function requiresSemiColon() {
        if (context.parent.type === "ForOfStatement" || context.parent.type === "ForInStatement")
            return context.parent.left !== node;

        return context.config["variableStatement.semiColon"] || context.parent.type === "ForStatement";
    }
}

function* parseVariableDeclarator(node: babel.VariableDeclarator, context: Context): PrintItemIterable {
    yield* parseNode(node.id, context);

    if (node.init) {
        yield " = ";
        yield* parseNode(node.init, context);
    }
}

/* class */

function parseClassBody(node: babel.ClassBody, context: Context): PrintItemIterable {
    const startHeaderInfo = context.bag.take(BAG_KEYS.ClassStartHeaderInfo) as Info | undefined;
    const bracePosition = context.parent.type === "ClassDeclaration"
        ? context.config["classDeclaration.bracePosition"]
        : context.config["classExpression.bracePosition"];

    return parseMemberedBody({
        bracePosition,
        context,
        members: node.body,
        node,
        startHeaderInfo,
        shouldUseBlankLine: (previousMember, nextMember) => {
            return nodeHelpers.hasSeparatingBlankLine(previousMember, nextMember);
        }
    });
}

function* parseClassOrObjectMethod(
    node: babel.ClassMethod | babel.TSDeclareMethod | babel.ObjectMethod,
    context: Context
): PrintItemIterable {
    if (node.type !== "ObjectMethod")
        yield* parseDecorators(node, context);

    const startHeaderInfo = createInfo("methodStartHeaderInfo");
    yield startHeaderInfo;

    if (node.type !== "ObjectMethod") {
        if (node.accessibility)
            yield node.accessibility + " ";
        if (node.static)
            yield "static ";
    }

    if (node.async)
        yield "async ";

    if (node.type !== "ObjectMethod" && node.abstract)
        yield "abstract ";

    if (node.kind === "get")
        yield "get ";
    else if (node.kind === "set")
        yield "set ";

    if (node.generator)
        yield "*";

    if (node.computed)
        yield "[";

    yield* parseNode(node.key, context);

    if (node.computed)
        yield "]";

    if (node.type !== "ObjectMethod" && node.optional)
        yield "?";

    if (node.typeParameters)
        yield* parseNode(node.typeParameters, context);

    yield* parseParametersOrArguments(node.params, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo: startHeaderInfo,
            typeNode: node.returnType
        })
    });

    if (node.type !== "TSDeclareMethod") {
        yield* parseBraceSeparator({
            bracePosition: context.config["classMethod.bracePosition"],
            bodyNode: node.body,
            startHeaderInfo: startHeaderInfo,
            context
        });
        yield* parseNode(node.body, context);
    }
    else if (context.config["classMethod.semiColon"]) {
        yield ";";
    }
}

function* parseClassProperty(node: babel.ClassProperty, context: Context): PrintItemIterable {
    yield* parseDecorators(node, context);

    if (node.accessibility)
        yield node.accessibility + " ";
    if (node.static)
        yield "static ";
    if (node.abstract)
        yield "abstract ";
    if (node.readonly)
        yield "readonly ";

    if (node.computed)
        yield "[";

    yield* parseNode(node.key, context);

    if (node.computed)
        yield "]";

    if (node.optional)
        yield "?";
    if (node.definite)
        yield "!";

    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);

    if (node.value) {
        yield " = ";
        yield* parseNode(node.value, context);
    }

    if (context.config["classProperty.semiColon"])
        yield ";";
}

function* parseDecorator(node: babel.Decorator, context: Context): PrintItemIterable {
    yield "@";
    yield* parseNode(node.expression, context);
}

function* parseParameterProperty(node: babel.TSParameterProperty, context: Context): PrintItemIterable {
    if (node.accessibility)
        yield node.accessibility + " ";
    if (node.readonly)
        yield "readonly ";

    yield* parseNode(node.parameter, context);
}

/* interface / type element */

function* parseCallSignatureDeclaration(node: babel.TSCallSignatureDeclaration, context: Context): PrintItemIterable {
    const startInfo = createInfo("startConstructSignature");
    yield startInfo;
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo,
            typeNode: node.typeAnnotation
        })
    });

    if (context.config["callSignature.semiColon"])
        yield ";";
}

function* parseConstructSignatureDeclaration(node: babel.TSConstructSignatureDeclaration, context: Context): PrintItemIterable {
    const startInfo = createInfo("startConstructSignature");
    yield startInfo;
    yield "new";
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo,
            typeNode: node.typeAnnotation
        })
    });

    if (context.config["constructSignature.semiColon"])
        yield ";";
}

function* parseIndexSignature(node: babel.TSIndexSignature, context: Context): PrintItemIterable {
    if (node.readonly)
        yield "readonly ";

    // todo: this should do something similar to the other declarations here (the ones with customCloseParen)
    yield "[";
    yield* parseNode(node.parameters[0], context);
    yield "]";
    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);

    if (context.config["indexSignature.semiColon"])
        yield ";";
}

function parseInterfaceBody(node: babel.TSInterfaceBody, context: Context): PrintItemIterable {
    const startHeaderInfo = context.bag.take(BAG_KEYS.InterfaceDeclarationStartHeaderInfo) as Info | undefined;

    return parseMemberedBody({
        bracePosition: context.config["interfaceDeclaration.bracePosition"],
        context,
        members: node.body,
        node,
        startHeaderInfo,
        shouldUseBlankLine: (previousMember, nextMember) => {
            return nodeHelpers.hasSeparatingBlankLine(previousMember, nextMember);
        }
    });
}

function* parseMethodSignature(node: babel.TSMethodSignature, context: Context): PrintItemIterable {
    const startInfo = createInfo("startConstructSignature");
    yield startInfo;

    if (node.computed)
        yield "[";

    yield* parseNode(node.key, context);

    if (node.computed)
        yield "]";

    if (node.optional)
        yield "?";

    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo,
            typeNode: node.typeAnnotation
        })
    });

    if (context.config["methodSignature.semiColon"])
        yield ";";
}

function* parsePropertySignature(node: babel.TSPropertySignature, context: Context): PrintItemIterable {
    if (node.readonly)
        yield "readonly ";

    if (node.computed)
        yield "[";

    yield* parseNode(node.key, context);

    if (node.computed)
        yield "]";

    if (node.optional)
        yield "?";

    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);

    if (node.initializer) {
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "= ";
            yield* parseNode(node.initializer, context);
        }());
    }

    if (context.config["propertySignature.semiColon"])
        yield ";";
}

/* module */

function parseModuleBlock(node: babel.TSModuleBlock, context: Context): PrintItemIterable {
    const startHeaderInfo = context.bag.take(BAG_KEYS.ModuleDeclarationStartHeaderInfo) as Info | undefined;

    return parseMemberedBody({
        bracePosition: context.config["moduleDeclaration.bracePosition"],
        context,
        members: node.body,
        node,
        startHeaderInfo,
        shouldUseBlankLine: (previousMember, nextMember) => {
            return nodeHelpers.hasSeparatingBlankLine(previousMember, nextMember);
        }
    });
}

/* statements */

function* parseBreakStatement(node: babel.BreakStatement, context: Context): PrintItemIterable {
    yield "break";

    if (node.label != null) {
        yield " ";
        yield* parseNode(node.label, context);
    }

    if (context.config["breakStatement.semiColon"])
        yield ";";
}

function* parseContinueStatement(node: babel.ContinueStatement, context: Context): PrintItemIterable {
    yield "continue";

    if (node.label != null) {
        yield " ";
        yield* parseNode(node.label, context);
    }

    if (context.config["continueStatement.semiColon"])
        yield ";";
}

function* parseDebuggerStatement(node: babel.DebuggerStatement, context: Context): PrintItemIterable {
    yield "debugger";
    if (context.config["debuggerStatement.semiColon"])
        yield ";";
}

function* parseDirective(node: babel.Directive, context: Context): PrintItemIterable {
    yield* parseNode(node.value, context);
    if (context.config["directive.semiColon"])
        yield ";";
}

function* parseDoWhileStatement(node: babel.DoWhileStatement, context: Context): PrintItemIterable {
    // the braces are technically optional on do while statements...
    yield "do";
    yield* parseBraceSeparator({
        bracePosition: context.config["doWhileStatement.bracePosition"],
        bodyNode: node.body,
        startHeaderInfo: undefined,
        context
    });
    yield* parseNode(node.body, context);
    yield " while ";
    yield* parseNodeInParens(node.test, context);

    if (context.config["doWhileStatement.semiColon"])
        yield ";";
}

function* parseEmptyStatement(node: babel.EmptyStatement, context: Context): PrintItemIterable {
    // Don't have configuration for this. Perhaps a change here would be
    // to not print anything for empty statements?
    yield ";";
}

function* parseExportAssignment(node: babel.TSExportAssignment, context: Context): PrintItemIterable {
    yield "export = ";
    yield* parseNode(node.expression, context);

    if (context.config["exportAssignment.semiColon"])
        yield ";";
}

function* parseExpressionStatement(node: babel.ExpressionStatement, context: Context): PrintItemIterable {
    yield* parseNode(node.expression, context);

    if (context.config["expressionStatement.semiColon"])
        yield ";";
}

function* parseForInStatement(node: babel.ForInStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "for ";
    yield "(";
    yield* parseInnerHeader();
    yield ")";
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forInStatement.useBraces"],
        bracePosition: context.config["forInStatement.bracePosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;

    function* parseInnerHeader(): PrintItemIterable {
        yield* parseNode(node.left, context);
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "in ";
            yield* parseNode(node.right, context);
        }());
    }
}

function* parseForOfStatement(node: babel.ForOfStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "for ";
    if (node.await)
        yield "await ";
    yield "(";
    yield* parseInnerHeader();
    yield ")";
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forOfStatement.useBraces"],
        bracePosition: context.config["forOfStatement.bracePosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;

    function* parseInnerHeader(): PrintItemIterable {
        yield* parseNode(node.left, context);
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "of ";
            yield* parseNode(node.right, context);
        }());
    }
}

function* parseForStatement(node: babel.ForStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "for (";
    yield* parseInnerHeader();
    yield ")";
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forStatement.useBraces"],
        bracePosition: context.config["forStatement.bracePosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;

    function* parseInnerHeader(): PrintItemIterable {
        yield* parseNode(node.init, context);
        if (!node.init || node.init.type !== "VariableDeclaration")
            yield ";";
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield* parseNode(node.test, context);
            yield ";";
        }());
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(parseNode(node.update, context));
    }
}

function* parseIfStatement(node: babel.IfStatement, context: Context): PrintItemIterable {
    const result = parseHeaderWithConditionalBraceBody({
        parseHeader: () => parseHeader(node),
        parent: node,
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

        // parse the leading comments before the else keyword
        yield* parseLeadingComments(node.alternate, context);

        yield "else";
        if (node.alternate.type === "IfStatement") {
            yield " ";
            yield* parseNode(node.alternate, context);
        }
        else {
            yield* parseConditionalBraceBody({
                parent: node,
                bodyNode: node.alternate,
                context,
                useBraces: context.config["ifStatement.useBraces"],
                bracePosition: context.config["ifStatement.bracePosition"],
                requiresBracesCondition: result.braceCondition
            }).iterator;
        }
    }

    function* parseHeader(ifStatement: babel.IfStatement): PrintItemIterable {
        yield "if ";
        yield* parseNodeInParens(ifStatement.test, context);
    }
}

function* parseInterpreterDirective(node: babel.InterpreterDirective, context: Context): PrintItemIterable {
    yield "#!";
    yield node.value;
}

function* parseLabeledStatement(node: babel.LabeledStatement, context: Context): PrintItemIterable {
    yield* parseNode(node.label, context);
    yield ":";

    // not bothering to make this configurable
    if (node.body.type === "BlockStatement")
        yield " ";
    else
        yield context.newlineKind;

    yield* parseNode(node.body, context);
}

function* parseReturnStatement(node: babel.ReturnStatement, context: Context): PrintItemIterable {
    yield "return";
    if (node.argument) {
        yield " ";
        yield* parseNode(node.argument, context);
    }

    if (context.config["returnStatement.semiColon"])
        yield ";";
}

function* parseSwitchCase(node: babel.SwitchCase, context: Context): PrintItemIterable {
    if (node.test == null)
        yield "default:";
    else {
        yield "case ";
        yield* parseNode(node.test, context);
        yield ":";
    }

    yield* parseFirstLineTrailingComments(node, node.consequent, context);

    if (node.consequent.length > 0) {
        yield context.newlineKind;

        yield* withIndent(parseStatementOrMembers({
            items: node.consequent,
            innerComments: node.innerComments,
            lastNode: undefined,
            context,
            shouldUseBlankLine: (previousNode, nextNode) => {
                return nodeHelpers.hasSeparatingBlankLine(previousNode, nextNode);
            }
        }));
    }
}

function* parseSwitchStatement(node: babel.SwitchStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    yield startHeaderInfo;
    yield "switch ";
    yield* parseNodeInParens(node.discriminant, context);

    yield* parseMemberedBody({
        bracePosition: context.config["switchStatement.bracePosition"],
        context,
        node,
        members: node.cases,
        startHeaderInfo,
        shouldUseBlankLine: () => false
    });
}

function* parseThrowStatement(node: babel.ThrowStatement, context: Context): PrintItemIterable {
    yield "throw ";
    yield* parseNode(node.argument, context);

    if (context.config["throwStatement.semiColon"])
        yield ";";
}

function* parseTryStatement(node: babel.TryStatement, context: Context): PrintItemIterable {
    yield "try";
    yield* parseBraceSeparator({
        bracePosition: context.config["tryStatement.bracePosition"],
        bodyNode: node.block,
        startHeaderInfo: undefined,
        context
    });
    yield* parseNode(node.block, context);

    if (node.handler != null) {
        yield* parseControlFlowSeparator(context.config["tryStatement.nextControlFlowPosition"], node.handler, "catch", context);
        yield* parseNode(node.handler, context);
    }

    if (node.finalizer != null) {
        yield* parseControlFlowSeparator(context.config["tryStatement.nextControlFlowPosition"], node.finalizer, "finally", context);
        yield "finally";
        yield* parseBraceSeparator({
            bracePosition: context.config["tryStatement.bracePosition"],
            bodyNode: node.finalizer,
            startHeaderInfo: undefined,
            context
        });
        yield* parseNode(node.finalizer, context);
    }
}

function* parseWhileStatement(node: babel.WhileStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "while ";
    yield* parseNodeInParens(node.test, context);
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["whileStatement.useBraces"],
        bracePosition: context.config["whileStatement.bracePosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;
}

/* clauses */

function* parseCatchClause(node: babel.CatchClause, context: Context): PrintItemIterable {
    // a bit overkill since the param will currently always be just an identifier
    const startHeaderInfo = createInfo("catchClauseHeaderStart");
    const endHeaderInfo = createInfo("catchClauseHeaderEnd");
    yield startHeaderInfo;
    yield "catch";
    if (node.param != null) {
        yield " (";
        yield* parseNode(node.param, context);
        yield ")";
    }

    // not conditional... required.
    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: "always",
        requiresBracesCondition: undefined,
        bracePosition: context.config["tryStatement.bracePosition"],
        startHeaderInfo,
        endHeaderInfo
    }).iterator;
}

interface ParseHeaderWithConditionalBraceBodyOptions {
    parent: babel.Node;
    bodyNode: babel.Statement;
    parseHeader(): PrintItemIterable;
    context: Context;
    requiresBracesCondition: Condition | undefined;
    useBraces: NonNullable<TypeScriptConfiguration["useBraces"]>;
    bracePosition: NonNullable<TypeScriptConfiguration["bracePosition"]>;
}

interface ParseHeaderWithConditionalBraceBodyResult {
    iterator: PrintItemIterable;
    braceCondition: Condition;
}

function parseHeaderWithConditionalBraceBody(opts: ParseHeaderWithConditionalBraceBodyOptions): ParseHeaderWithConditionalBraceBodyResult {
    const { context, parent, bodyNode, requiresBracesCondition, useBraces, bracePosition } = opts;
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");

    const result = parseConditionalBraceBody({
        context,
        parent,
        bodyNode,
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
    };

    function* parseHeader(): PrintItemIterable {
        yield startHeaderInfo;
        yield* opts.parseHeader();
        yield endHeaderInfo;
    }
}

interface ParseConditionalBraceBodyOptions {
    parent: babel.Node;
    bodyNode: babel.Statement;
    context: Context;
    useBraces: NonNullable<TypeScriptConfiguration["useBraces"]>;
    bracePosition: NonNullable<TypeScriptConfiguration["bracePosition"]>;
    requiresBracesCondition: Condition | undefined;
    startHeaderInfo?: Info;
    endHeaderInfo?: Info;
}

interface ParseConditionalBraceBodyResult {
    iterator: PrintItemIterable;
    braceCondition: Condition;
}

function parseConditionalBraceBody(opts: ParseConditionalBraceBodyOptions): ParseConditionalBraceBodyResult {
    const { startHeaderInfo, endHeaderInfo, parent, bodyNode, context, requiresBracesCondition, useBraces, bracePosition } = opts;
    const startStatementsInfo = createInfo("startStatements");
    const endStatementsInfo = createInfo("endStatements");
    const headerTrailingComments = Array.from(getHeaderTrailingComments());
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
                return bodyRequiresBraces()
                    || startHeaderInfo && endHeaderInfo && conditionResolvers.isMultipleLines(conditionContext, startHeaderInfo, endHeaderInfo, false)
                    || conditionResolvers.isMultipleLines(conditionContext, startStatementsInfo, endStatementsInfo, false)
                    || requiresBracesCondition && conditionContext.getResolvedCondition(requiresBracesCondition);
            }
            else {
                return assertNever(useBraces);
            }
        },
        true: function*() {
            yield* parseBraceSeparator({
                bracePosition,
                bodyNode,
                startHeaderInfo,
                context
            });
            yield "{";
        }()
    };

    return {
        braceCondition: openBraceCondition,
        iterator: parseBody()
    };

    function* parseBody(): PrintItemIterable {
        yield openBraceCondition;

        yield* parseHeaderTrailingComment();

        yield context.newlineKind;
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
        else {
            yield* withIndent(function*() {
                yield* parseNode(bodyNode, context);
                // When there's no body and this is the last control flow,
                // the parent's trailing comments are actually this line's
                // trailing comment.
                if (bodyNode.end === parent.end)
                    yield* parseTrailingComments(parent, context);
            }());
        }

        yield endStatementsInfo;
        yield {
            kind: PrintItemKind.Condition,
            name: "closeBrace",
            condition: openBraceCondition,
            true: [{
                kind: PrintItemKind.Condition,
                name: "closeBraceNewLine",
                condition: conditionContext => {
                    return !conditionResolvers.areInfoEqual(conditionContext, startStatementsInfo, endStatementsInfo, false);
                },
                true: [context.newlineKind]
            }, "}"]
        };

        function* parseHeaderTrailingComment(): PrintItemIterable {
            const result = parseCommentCollection(headerTrailingComments, undefined, context);
            yield* prependToIterableIfHasItems(result, " "); // add a space
        }
    }

    function bodyRequiresBraces() {
        if (bodyNode.type === "BlockStatement") {
            if (bodyNode.body.length === 1 && !nodeHelpers.hasLeadingCommentOnDifferentLine(bodyNode.body[0], /* commentsToIgnore */ headerTrailingComments))
                return false;
            return true;
        }

        return nodeHelpers.hasLeadingCommentOnDifferentLine(bodyNode, /* commentsToIgnore */ headerTrailingComments);
    }

    function* getHeaderTrailingComments() {
        if (bodyNode.type === "BlockStatement") {
            if (bodyNode.leadingComments != null) {
                const commentLine = bodyNode.leadingComments.find(c => c.type === "CommentLine");
                if (commentLine) {
                    yield commentLine;
                    return;
                }
            }

            if (bodyNode.body.length > 0)
                yield* checkComments(bodyNode.body[0].leadingComments);
            else if (bodyNode.innerComments)
                yield* checkComments(bodyNode.innerComments);
        }
        else {
            if (bodyNode.leadingComments && bodyNode.leadingComments.length > 0) {
                const lastHeaderToken = tokenHelpers.getFirstNonCommentTokenBefore(bodyNode, context)!;
                for (const comment of bodyNode.leadingComments) {
                    if (comment.loc.start.line <= lastHeaderToken.loc!.end.line)
                        yield comment;
                }
            }
        }

        function* checkComments(comments: ReadonlyArray<babel.Comment> | null | undefined) {
            if (comments == null)
                return;

            for (const comment of comments) {
                if (comment.loc.start.line === bodyNode.loc!.start.line)
                    yield comment;
            }
        }
    }
}

/* expressions */

function* parseArrayPattern(node: babel.ArrayPattern, context: Context): PrintItemIterable {
    yield* parseArrayLikeNodes({
        node,
        elements: node.elements,
        trailingCommas: context.config["arrayPattern.trailingCommas"],
        context
    });
    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);
}

function* parseArrayExpression(node: babel.ArrayExpression, context: Context): PrintItemIterable {
    yield* parseArrayLikeNodes({
        node,
        elements: node.elements,
        trailingCommas: context.config["arrayExpression.trailingCommas"],
        context
    });
}

function* parseArrowFunctionExpression(node: babel.ArrowFunctionExpression, context: Context): PrintItemIterable {
    const headerStartInfo = createInfo("functionExpressionHeaderStart");
    yield headerStartInfo;

    if (node.async)
        yield "async ";

    yield* parseNode(node.typeParameters, context);

    // todo: configuration (issue #7)
    if (node.params.length !== 1 || hasParentheses() || node.returnType) {
        yield* parseParametersOrArguments(node.params, context, {
            customCloseParen: parseCloseParenWithType({
                context,
                startInfo: headerStartInfo,
                typeNode: node.returnType
            })
        });
    }
    else {
        yield* parseNode(node.params[0], context);
    }

    yield " =>";

    yield* parseBraceSeparator({
        bracePosition: context.config["arrowFunctionExpression.bracePosition"],
        bodyNode: node.body,
        startHeaderInfo: headerStartInfo,
        context
    });

    yield* parseNode(node.body, context);

    function hasParentheses() {
        if (node.params.length !== 1)
            return true;

        return context.tokenFinder.isFirstTokenInNodeMatch(node, "(");
    }
}

function* parseAsExpression(node: babel.TSAsExpression, context: Context): PrintItemIterable {
    yield* parseNode(node.expression, context);
    yield " as ";
    yield* conditions.withIndentIfStartOfLineIndented(parseNode(node.typeAnnotation, context));
}

function* parseAssignmentExpression(node: babel.AssignmentExpression, context: Context): PrintItemIterable {
    yield* parseNode(node.left, context);
    yield ` ${node.operator} `;
    yield* conditions.withIndentIfStartOfLineIndented(parseNode(node.right, context));
}

function* parseAssignmentPattern(node: babel.AssignmentPattern, context: Context): PrintItemIterable {
    yield* newlineGroup(function*() {
        yield* parseNode(node.left, context);
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "= ";
            yield* parseNode(node.right, context);
        }());
    }());
}

function* parseAwaitExpression(node: babel.AwaitExpression, context: Context): PrintItemIterable {
    yield "await ";
    yield* parseNode(node.argument, context);
}

function* parseBinaryOrLogicalExpression(node: babel.LogicalExpression | babel.BinaryExpression, context: Context): PrintItemIterable {
    const shouldIndent = context.bag.take(BAG_KEYS.DisableIndentBool) == null;
    const useNewLines = getUseNewLines();
    const wasLastSame = context.parent.type === node.type;

    if (wasLastSame)
        yield* parseInner();
    else
        yield* newlineGroup(parseInner());

    function* parseInner(): PrintItemIterable {
        if (!shouldIndent)
            putDisableIndentInBagIfNecessaryForNode(node.left, context);

        yield* parseNode(node.left, context);

        if (useNewLines)
            yield context.newlineKind;
        else
            yield Signal.SpaceOrNewLine;

        if (!shouldIndent)
            putDisableIndentInBagIfNecessaryForNode(node.right, context);

        const rightIterator = parseNode(node.right, context, {
            innerParse: function*(iterator) {
                yield node.operator;
                yield " ";
                yield* iterator;
            }
        });

        yield* shouldIndent ? conditions.indentIfStartOfLine(rightIterator) : rightIterator;
    }

    function getUseNewLines() {
        return nodeHelpers.getUseNewlinesForNodes([getLeftNode(), getRightNode()]);

        function getLeftNode() {
            const hasParentheses = nodeHelpers.hasParentheses(node.left);
            return hasParentheses ? tokenHelpers.getFirstCloseParenTokenAfter(node.left, context)! : node.left;
        }
        function getRightNode() {
            const hasParentheses = nodeHelpers.hasParentheses(node.right);
            return hasParentheses ? tokenHelpers.getFirstOpenParenTokenBefore(node.right, context)! : node.right;
        }
    }
}

function* parseExpressionWithTypeArguments(node: babel.TSExpressionWithTypeArguments, context: Context): PrintItemIterable {
    yield* parseNode(node.expression, context);
    yield* parseNode(node.typeParameters, context); // arguments, not parameters
}

function* parseExternalModuleReference(node: babel.TSExternalModuleReference, context: Context): PrintItemIterable {
    yield "require(";
    yield* parseNode(node.expression, context);
    yield ")";
}

function* parseCallExpression(node: babel.CallExpression | babel.OptionalCallExpression, context: Context): PrintItemIterable {
    if (isTestLibraryCallExpression())
        yield* parseTestLibraryCallExpression();
    else
        yield* innerParseCallExpression();

    function* innerParseCallExpression(): PrintItemIterable {
        yield* parseNode(node.callee, context);

        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        if (node.optional)
            yield "?.";

        yield* conditions.withIndentIfStartOfLineIndented(parseParametersOrArguments(node.arguments, context));
    }

    function* parseTestLibraryCallExpression(): PrintItemIterable {
        yield* parseTestLibraryCallee();
        yield* parseTestLibraryArguments();

        function* parseTestLibraryCallee(): PrintItemIterable {
            if (node.callee.type === "MemberExpression") {
                yield* parseNode(node.callee.object, context);
                yield ".";
                yield* parseNode(node.callee.property, context);
            }
            else {
                yield* parseNode(node.callee, context); // identifier
            }
        }

        function* parseTestLibraryArguments(): PrintItemIterable {
            yield "(";
            yield* parseNode(node.arguments[0], context, {
                innerParse: function*(iterator) {
                    yield* stripSignals(iterator);
                    yield ",";
                }
            });
            yield " ";
            yield* parseNode(node.arguments[1], context);
            yield ")";
        }

        /** Stop the iterator from providing any formatting information (ex. Signal.NewLine). */
        function* stripSignals(iterator: PrintItemIterable): PrintItemIterable {
            // If this function is used more generally, it should also strip
            // signal information from conditions.
            for (const item of iterator) {
                if (typeof item !== "number")
                    yield item;
            }
        }
    }

    /**
     * Tests if this is a call expression from common test libraries.
     * Be very strict here to allow the user to opt out if they'd like.
     */
    function isTestLibraryCallExpression() {
        if (node.arguments.length !== 2 || node.typeArguments != null || node.optional)
            return false;
        if (!isValidCallee())
            return false;
        if (node.arguments[0].type !== "StringLiteral" && node.arguments[0].type !== "TemplateLiteral")
            return false;
        if (node.arguments[1].type !== "FunctionExpression" && node.arguments[1].type !== "ArrowFunctionExpression")
            return false;

        return node.loc!.start.line === node.arguments[1].loc!.start.line;

        function isValidCallee() {
            const identifier = getIdentifier();
            if (identifier == null)
                return false;
            switch (identifier.name) {
                case "it":
                case "describe":
                    return true;
                default:
                    return false;
            }

            function getIdentifier() {
                if (node.callee.type === "Identifier")
                    return node.callee;
                if (
                    node.callee.type === "MemberExpression"
                    && node.callee.object.type === "Identifier"
                    && node.callee.property.type === "Identifier"
                ) {
                    return node.callee.object;
                }

                return undefined;
            }
        }
    }
}

function* parseConditionalExpression(node: babel.ConditionalExpression, context: Context): PrintItemIterable {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes([node.test, node.consequent])
        || nodeHelpers.getUseNewlinesForNodes([node.consequent, node.alternate]);
    const startInfo = createInfo("startConditionalExpression");
    const afterAlternateColonInfo = createInfo("afterAlternateColon");
    const endInfo = createInfo("endConditionalExpression");

    yield startInfo;
    yield* newlineGroup(parseNode(node.test, context));
    yield* parseConsequentAndAlternate();

    function* parseConsequentAndAlternate(): PrintItemIterable {
        // force re-evaluation of all the conditions below
        // once the endInfo has been reached
        yield conditions.forceReevaluationOnceResolved(context.endStatementOrMemberInfo.peek() || endInfo);

        if (useNewlines)
            yield context.newlineKind;
        else
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo, afterAlternateColonInfo);

        yield* conditions.indentIfStartOfLine(function*() {
            yield "? ";
            yield* newlineGroup(parseNode(node.consequent, context));
        }());

        if (useNewlines)
            yield context.newlineKind;
        else
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo, afterAlternateColonInfo);

        yield* conditions.indentIfStartOfLine(function*() {
            yield ": ";
            yield afterAlternateColonInfo;
            yield* newlineGroup(parseNode(node.alternate, context));
            yield endInfo;
        }());
    }
}

function* parseMemberExpression(node: babel.MemberExpression, context: Context): PrintItemIterable {
    yield* parseForMemberLikeExpression(node.object, node.property, node.computed, context);
}

function* parseMetaProperty(node: babel.MetaProperty, context: Context): PrintItemIterable {
    yield* parseForMemberLikeExpression(node.meta, node.property, false, context);
}

function* parseNewExpression(node: babel.NewExpression, context: Context): PrintItemIterable {
    yield "new ";
    yield* parseNode(node.callee, context);
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.arguments, context);
}

function* parseNonNullExpression(node: babel.TSNonNullExpression, context: Context): PrintItemIterable {
    yield* parseNode(node.expression, context);
    yield "!";
}

function* parseObjectExpression(node: babel.ObjectExpression, context: Context): PrintItemIterable {
    yield* parseObjectLikeNode({
        node,
        members: node.properties,
        context,
        trailingCommas: context.config["objectExpression.trailingCommas"]
    });
}

function* parseObjectPattern(node: babel.ObjectPattern, context: Context): PrintItemIterable {
    yield* parseObjectLikeNode({
        node,
        members: node.properties,
        context,
        trailingCommas: "never"
    });
    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);
}

function* parseObjectProperty(node: babel.ObjectProperty, context: Context): PrintItemIterable {
    if (!node.shorthand) {
        if (node.computed)
            yield "[";

        yield* parseNode(node.key, context);

        if (node.computed)
            yield "]";
    }

    if (node.value) {
        if (node.shorthand)
            yield* parseNode(node.value, context);
        else
            yield* parseNodeWithPreceedingColon(node.value, context);
    }
}

function* parseRestElement(node: babel.RestElement, context: Context): PrintItemIterable {
    yield "...";
    yield* parseNode(node.argument, context);
    yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);
}

function* parseSpreadElement(node: babel.SpreadElement, context: Context): PrintItemIterable {
    yield "...";
    yield* parseNode(node.argument, context);
}

function* parseTaggedTemplateExpression(node: babel.TaggedTemplateExpression, context: Context): PrintItemIterable {
    yield* newlineGroup(function*() {
        yield* parseNode(node.tag, context);
        yield* parseNode(node.typeParameters, context);
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(parseNode(node.quasi, context));
    }());
}

function* parseTypeAssertion(node: babel.TSTypeAssertion, context: Context): PrintItemIterable {
    yield "<";
    yield* parseNode(node.typeAnnotation, context);
    yield "> ";
    yield* parseNode(node.expression, context);
}

function* parseUnaryExpression(node: babel.UnaryExpression, context: Context): PrintItemIterable {
    const operator = getOperator();
    if (node.prefix)
        yield operator;

    yield* parseNode(node.argument, context);

    if (!node.prefix)
        yield operator;

    function getOperator() {
        switch (node.operator) {
            case "void":
            case "typeof":
            case "throw":
            case "delete":
                return `${node.operator} `;
            case "!":
            case "+":
            case "-":
            case "~":
                return node.operator;
            default:
                const assertNever: never = node.operator;
                return node.operator;
        }
    }
}

function* parseUpdateExpression(node: babel.UpdateExpression, context: Context): PrintItemIterable {
    if (node.prefix)
        yield node.operator;

    yield* parseNode(node.argument, context);

    if (!node.prefix)
        yield node.operator;
}

function* parseYieldExpression(node: babel.YieldExpression, context: Context): PrintItemIterable {
    yield "yield";
    if (node.delegate)
        yield "*";
    yield " ";
    yield* parseNode(node.argument, context);
}

/* imports */

function parseImportDefaultSpecifier(specifier: babel.ImportDefaultSpecifier, context: Context) {
    return parseNode(specifier.local, context);
}

function* parseImportNamespaceSpecifier(specifier: babel.ImportNamespaceSpecifier, context: Context): PrintItemIterable {
    yield "* as ";
    yield* parseNode(specifier.local, context);
}

function* parseImportSpecifier(specifier: babel.ImportSpecifier, context: Context): PrintItemIterable {
    if (specifier.imported.start === specifier.local.start) {
        yield* parseNode(specifier.imported, context);
        return;
    }

    yield* parseNode(specifier.imported, context);
    yield " as ";
    yield* parseNode(specifier.local, context);
}

/* exports */

function* parseExportDefaultSpecifier(node: babel.ExportDefaultSpecifier, context: Context): PrintItemIterable {
    yield "default ";
    yield* parseNode(node.exported, context);
}

function* parseExportNamespaceSpecifier(node: babel.ExportNamespaceSpecifier, context: Context): PrintItemIterable {
    yield "* as ";
    yield* parseNode(node.exported, context);
}

function* parseExportSpecifier(specifier: babel.ExportSpecifier, context: Context): PrintItemIterable {
    if (specifier.local.start === specifier.exported.start) {
        yield* parseNode(specifier.local, context);
        return;
    }

    yield* parseNode(specifier.local, context);
    yield " as ";
    yield* parseNode(specifier.exported, context);
}

/* literals */

function* parseBigIntLiteral(node: babel.BigIntLiteral, context: Context): PrintItemIterable {
    yield node.value + "n";
}

function* parseBooleanLiteral(node: babel.BooleanLiteral, context: Context): PrintItemIterable {
    yield node.value ? "true" : "false";
}

function* parseNumericLiteral(node: babel.NumericLiteral, context: Context): PrintItemIterable {
    yield context.fileText.substring(node.start!, node.end!);
}

function* parseStringOrDirectiveLiteral(node: babel.StringLiteral | babel.DirectiveLiteral, context: Context): PrintItemIterable {
    yield {
        kind: PrintItemKind.RawString,
        text: getStringLiteralText()
    };

    function getStringLiteralText() {
        const stringValue = getStringValue();

        if (context.config.singleQuotes)
            return `'${stringValue.replace(/'/g, `\\'`)}'`;
        else
            return `"${stringValue.replace(/"/g, `\\"`)}"`;

        function getStringValue() {
            // do not use node.value because it will not keep escaped characters as escaped characters
            const rawStringValue = context.fileText.substring(node.start! + 1, node.end! - 1);
            const isDoubleQuote = context.fileText[node.start!] === `"`;

            if (isDoubleQuote)
                return rawStringValue.replace(/\\"/g, `"`);
            else
                return rawStringValue.replace(/\\'/g, `'`);
        }
    }
}

function* parseRegExpLiteral(node: babel.RegExpLiteral, context: Context): PrintItemIterable {
    yield "/";
    yield node.pattern;
    yield "/";
    yield node.flags;
}

function* parseTemplateElement(node: babel.TemplateElement, context: Context): PrintItemIterable {
    yield {
        kind: PrintItemKind.RawString,
        text: context.fileText.substring(node.start!, node.end!)
    };
}

function* parseTemplateLiteral(node: babel.TemplateLiteral, context: Context): PrintItemIterable {
    yield* newlineGroup(function*() {
        yield "`";
        yield Signal.StartIgnoringIndent;
        for (const item of getItems()) {
            if (item.type === "TemplateElement")
                yield* parseNode(item, context);
            else {
                yield "${";
                yield Signal.FinishIgnoringIndent;
                yield Signal.NewLine;
                yield conditions.singleIndentIfStartOfLine();
                yield* parseNode(item, context);
                yield Signal.NewLine;
                yield conditions.singleIndentIfStartOfLine();
                yield "}";
                yield Signal.StartIgnoringIndent;
            }
        }
        yield "`";
        yield Signal.FinishIgnoringIndent;
    }());

    function* getItems(): Iterable<babel.Node> {
        let quasisIndex = 0;
        let expressionsIndex = 0;

        while (true) {
            const currentQuasis = node.quasis[quasisIndex];
            const currentExpression = node.expressions[expressionsIndex];

            if (currentQuasis != null) {
                if (currentExpression != null) {
                    if (currentQuasis.start! < currentExpression.start!)
                        yield moveNextQuasis();
                    else
                        yield moveNextExpression();
                }
                else {
                    yield moveNextQuasis();
                }
            }
            else if (currentExpression != null)
                yield moveNextExpression();
            else
                return;

            function moveNextQuasis() {
                quasisIndex++;
                return currentQuasis;
            }

            function moveNextExpression() {
                expressionsIndex++;
                return currentExpression;
            }
        }
    }
}

/* not implemented */

function parseNotSupportedFlowNode(node: babel.Node, context: Context): PrintItemIterable {
    return toPrintItemIterable(parseUnknownNodeWithMessage(node, context, "Flow node types are not supported"));
}

function parseUnknownNode(node: babel.Node, context: Context): PrintItemIterable {
    return toPrintItemIterable(parseUnknownNodeWithMessage(node, context, "Not implemented node type"));
}

function parseUnknownNodeWithMessage(node: babel.Node, context: Context, message: string): RawString {
    const nodeText = context.fileText.substring(node.start!, node.end!);

    context.log(`${message}: ${node.type} (${nodeText.substring(0, 100)})`);

    return {
        kind: PrintItemKind.RawString,
        text: nodeText
    };
}

/* types */

function* parseArrayType(node: babel.TSArrayType, context: Context): PrintItemIterable {
    yield* newlineGroup(parseNode(node.elementType, context));
    yield "[]";
}

function* parseConditionalType(node: babel.TSConditionalType, context: Context): PrintItemIterable {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes([node.checkType, node.falseType]);
    const isParentConditionalType = context.parent.type === "TSConditionalType";

    yield* newlineGroup(parseMainArea());
    yield* parseFalseType();

    function* parseMainArea(): PrintItemIterable {
        yield* newlineGroup(parseNode(node.checkType, context));
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "extends ";
            yield* newlineGroup(parseNode(node.extendsType, context));
        }());
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "? ";
            yield* newlineGroup(parseNode(node.trueType, context));
        }());
    }

    function* parseFalseType(): PrintItemIterable {
        if (useNewlines)
            yield context.newlineKind;
        else
            yield Signal.SpaceOrNewLine;

        if (isParentConditionalType)
            yield* parseInner();
        else
            yield* conditions.indentIfStartOfLine(parseInner());

        function* parseInner(): PrintItemIterable {
            yield ": ";
            yield* newlineGroup(parseNode(node.falseType, context));
        }
    }
}

function* parseConstructorType(node: babel.TSConstructorType, context: Context): PrintItemIterable {
    const startInfo = createInfo("startConstructorType");
    yield startInfo;
    yield "new";
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo,
            typeNode: node.typeAnnotation,
            typeNodeSeparator: function*() {
                yield Signal.SpaceOrNewLine;
                yield "=> ";
            }()
        })
    });
}

function* parseFunctionType(node: babel.TSFunctionType, context: Context): PrintItemIterable {
    const startInfo = createInfo("startConstructorType");
    yield startInfo;
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context, {
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo,
            typeNode: node.typeAnnotation,
            typeNodeSeparator: function*() {
                yield Signal.SpaceOrNewLine;
                yield "=> ";
            }()
        })
    });
}

function* parseImportType(node: babel.TSImportType, context: Context): PrintItemIterable {
    yield "import(";
    yield* parseNode(node.argument, context);
    yield ")";

    if (node.qualifier) {
        yield ".";
        yield* parseNode(node.qualifier, context);
    }

    // incorrectly named... these are type arguments!
    yield* parseNode(node.typeParameters, context);
}

function* parseIndexedAccessType(node: babel.TSIndexedAccessType, context: Context): PrintItemIterable {
    yield* parseNode(node.objectType, context);
    yield "[";
    yield* parseNode(node.indexType, context);
    yield "]";
}

function* parseInferType(node: babel.TSInferType, context: Context): PrintItemIterable {
    yield "infer ";
    yield* parseNode(node.typeParameter, context);
}

function* parseLiteralType(node: babel.TSLiteralType, context: Context): PrintItemIterable {
    yield* parseNode(node.literal, context);
}

function* parseMappedType(node: babel.TSMappedType, context: Context): PrintItemIterable {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes([tokenHelpers.getFirstOpenBraceTokenWithin(node, context), node.typeParameter]);
    const startInfo = createInfo("startMappedType");
    yield startInfo;
    yield "{";

    yield* parseLayout();

    yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo);
    yield "}";

    function* parseLayout(): PrintItemIterable {
        if (useNewLines)
            yield context.newlineKind;
        else
            yield Signal.SpaceOrNewLine;

        yield* conditions.indentIfStartOfLine(newlineGroup(parseBody()));
    }

    function* parseBody(): PrintItemIterable {
        if (node.readonly)
            yield "readonly ";

        yield "[";
        yield* parseNode(node.typeParameter, context);
        yield "]";
        if (node.optional)
            yield "?";

        yield* parseTypeAnnotationWithColonIfExists(node.typeAnnotation, context);

        if (context.config["mappedType.semiColon"])
            yield ";";
    }
}

function* parseOptionalType(node: babel.TSOptionalType, context: Context): PrintItemIterable {
    yield* parseNode(node.typeAnnotation, context);
    yield "?";
}

function* parseParenthesizedType(node: babel.TSParenthesizedType, context: Context): PrintItemIterable {
    yield* conditions.withIndentIfStartOfLineIndented(parseNodeInParens(node.typeAnnotation, context));
}

function* parseQualifiedName(node: babel.TSQualifiedName, context: Context): PrintItemIterable {
    yield* parseNode(node.left, context);
    yield ".";
    yield* parseNode(node.right, context);
}

function* parseRestType(node: babel.TSRestType, context: Context): PrintItemIterable {
    yield "...";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTupleType(node: babel.TSTupleType, context: Context): PrintItemIterable {
    const useNewlines = getUseNewLines();
    const forceTrailingCommas = getForceTrailingCommas(context.config["tupleType.trailingCommas"], useNewlines);

    yield "[";

    if (node.elementTypes.length > 0)
        yield* parseElements();

    yield "]";

    function* parseElements(): PrintItemIterable {
        if (useNewlines)
            yield context.newlineKind;

        for (let i = 0; i < node.elementTypes.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            yield* conditions.indentIfStartOfLine(parseNode(node.elementTypes[i], context, {
                innerParse: function*(iterator) {
                    yield* iterator;

                    if (forceTrailingCommas || i < node.elementTypes.length - 1)
                        yield ",";
                }
            }));

            if (useNewlines)
                yield context.newlineKind;
        }
    }

    function getUseNewLines() {
        if (node.elementTypes.length === 0)
            return false;

        return nodeHelpers.getUseNewlinesForNodes([
            tokenHelpers.getFirstOpenBracketTokenWithin(node, context),
            node.elementTypes[0]
        ]);
    }
}

function* parseTypeAnnotation(node: babel.TSTypeAnnotation, context: Context): PrintItemIterable {
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeLiteral(node: babel.TSTypeLiteral, context: Context): PrintItemIterable {
    yield* parseObjectLikeNode({
        node,
        members: node.members,
        context
    });
}

function* parseTypeOperator(node: babel.TSTypeOperator, context: Context): PrintItemIterable {
    if (node.operator)
        yield `${node.operator} `;

    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeParameter(node: babel.TSTypeParameter, context: Context): PrintItemIterable {
    yield node.name!;

    if (node.constraint) {
        if (context.parent.type === "TSMappedType")
            yield " in ";
        else
            yield " extends ";

        yield* parseNode(node.constraint, context);
    }

    if (node.default) {
        yield " = ";
        yield* parseNode(node.default, context);
    }
}

function* parseTypePredicate(node: babel.TSTypePredicate, context: Context): PrintItemIterable {
    yield* parseNode(node.parameterName, context);
    yield " is ";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeQuery(node: babel.TSTypeQuery, context: Context): PrintItemIterable {
    yield "typeof ";
    yield* parseNode(node.exprName, context);
}

function* parseTypeReference(node: babel.TSTypeReference, context: Context): PrintItemIterable {
    yield* parseNode(node.typeName, context);
    yield* parseNode(node.typeParameters, context);
}

function* parseUnionOrIntersectionType(node: babel.TSUnionType | babel.TSIntersectionType, context: Context): PrintItemIterable {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes(node.types);
    const separator = node.type === "TSUnionType" ? "| " : "& ";
    const isAncestorParenthesizedType = getIsAncestorParenthesizedType();

    for (let i = 0; i < node.types.length; i++) {
        if (i > 0)
            yield useNewLines ? context.newlineKind : Signal.SpaceOrNewLine;

        // probably something better needs to be done here, but this is good enough for now
        if (isAncestorParenthesizedType)
            yield* innerParse(i);
        else
            yield* conditions.indentIfStartOfLine(innerParse(i));
    }

    function* innerParse(index: number): PrintItemIterable {
        if (index > 0)
            yield separator;

        yield* parseNode(node.types[index], context);
    }

    function getIsAncestorParenthesizedType() {
        for (let i = context.parentStack.length - 1; i >= 0; i--) {
            switch (context.parentStack[i].type) {
                case "TSUnionType":
                case "TSIntersectionType":
                    continue;
                case "TSParenthesizedType":
                    return true;
                default:
                    return false;
            }
        }

        return false;
    }
}

/* general */

interface ParseMemberedBodyOptions {
    node: babel.Node;
    members: babel.Node[];
    context: Context;
    startHeaderInfo: Info | undefined;
    bracePosition: NonNullable<TypeScriptConfiguration["bracePosition"]>;
    shouldUseBlankLine: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
    trailingCommas?: TypeScriptConfiguration["trailingCommas"];
}

function* parseMemberedBody(opts: ParseMemberedBodyOptions): PrintItemIterable {
    const { node, members, context, startHeaderInfo, bracePosition, shouldUseBlankLine, trailingCommas } = opts;

    yield* parseBraceSeparator({
        bracePosition,
        bodyNode: tokenHelpers.getFirstOpenBraceTokenWithin(node, context) || node,
        startHeaderInfo,
        context
    });

    yield "{";
    yield* parseFirstLineTrailingComments(node, members, context);
    yield* withIndent(parseBody());
    yield context.newlineKind;
    yield "}";

    function* parseBody(): PrintItemIterable {
        // todo: remove filter—don't allocate a new array for this.
        if (members.length > 0 || node.innerComments != null && node.innerComments.filter(n => !context.handledComments.has(n)).length > 0)
            yield context.newlineKind;

        yield* parseStatementOrMembers({
            items: members,
            innerComments: node.innerComments,
            lastNode: undefined,
            context,
            shouldUseBlankLine,
            trailingCommas
        });
    }
}

function* parseStatements(block: babel.BlockStatement | babel.Program, context: Context): PrintItemIterable {
    let lastNode: babel.Node | undefined;
    for (const directive of block.directives) {
        if (lastNode != null) {
            yield context.newlineKind;
            if (nodeHelpers.hasSeparatingBlankLine(lastNode, directive))
                yield context.newlineKind;
        }

        yield* parseNode(directive, context);
        lastNode = directive;
    }

    const statements = block.body;
    yield* parseStatementOrMembers({
        items: statements,
        innerComments: block.innerComments,
        lastNode,
        context,
        shouldUseBlankLine: (previousStatement, nextStatement) => {
            return nodeHelpers.hasSeparatingBlankLine(previousStatement, nextStatement);
        }
    });
}

interface ParseStatementOrMembersOptions {
    items: babel.Node[];
    innerComments: ReadonlyArray<babel.Comment> | undefined | null;
    lastNode: babel.Node | undefined;
    context: Context;
    shouldUseBlankLine: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
    trailingCommas?: TypeScriptConfiguration["trailingCommas"];
}

function* parseStatementOrMembers(opts: ParseStatementOrMembersOptions): PrintItemIterable {
    const { items, innerComments, context, shouldUseBlankLine, trailingCommas } = opts;
    let { lastNode } = opts;

    for (const item of items) {
        if (lastNode != null) {
            yield context.newlineKind;

            if (shouldUseBlankLine(lastNode, item))
                yield context.newlineKind;
        }

        const endInfo = createInfo("endStatementOrMemberInfo");
        context.endStatementOrMemberInfo.push(endInfo);
        yield* parseNode(item, context, {
            innerParse: function*(iterator) {
                yield* iterator;

                if (trailingCommas) {
                    const forceTrailingCommas = getForceTrailingCommas(trailingCommas, true);
                    if (forceTrailingCommas || items[items.length - 1] !== item)
                        yield ",";
                }
            }
        });
        yield context.endStatementOrMemberInfo.popOrThrow();

        lastNode = item;
    }

    // get the trailing comments on separate lines of the last node
    if (lastNode != null && lastNode.trailingComments != null) {
        const unHandledComments = lastNode.trailingComments.filter(c => !context.handledComments.has(c));
        if (unHandledComments.length > 0) {
            yield context.newlineKind;

            if (nodeHelpers.hasSeparatingBlankLine(lastNode, unHandledComments[0]))
                yield context.newlineKind;
            // treat these as if they were leading comments, so don't provide the last node
            yield* parseCommentCollection(lastNode.trailingComments, undefined, context);
        }
    }

    if (innerComments != null && innerComments.length > 0) {
        if (lastNode != null)
            yield context.newlineKind;

        yield* parseCommentCollection(innerComments, undefined, context);
    }
}

interface ParseParametersOrArgumentsOptions {
    customCloseParen?: PrintItemIterable;
}

function* parseParametersOrArguments(params: babel.Node[], context: Context, options: ParseParametersOrArgumentsOptions = {}): PrintItemIterable {
    const { customCloseParen } = options;
    const useNewLines = getUseNewLines();
    yield* newlineGroup(parseItems());

    function* parseItems(): PrintItemIterable {
        yield "(";

        if (useNewLines)
            yield* surroundWithNewLines(withIndent(parseParameterList()), context);
        else
            yield* parseParameterList();

        if (customCloseParen)
            yield* customCloseParen;
        else
            yield ")";
    }

    function* parseParameterList(): PrintItemIterable {
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            const hasComma = i < params.length - 1;
            const parsedParam = parseParam(param, hasComma);

            if (i === 0)
                yield* parsedParam;
            else if (useNewLines) {
                yield context.newlineKind;
                yield* parsedParam;
            }
            else {
                yield Signal.SpaceOrNewLine;
                yield* conditions.indentIfStartOfLine(parsedParam);
            }
        }

        function* parseParam(param: babel.Node, hasComma: boolean): PrintItemIterable {
            yield* newlineGroup(parseNode(param, context, {
                innerParse: function*(iterator) {
                    yield* iterator;

                    if (hasComma)
                        yield ",";
                }
            }));
        }
    }

    function getUseNewLines() {
        if (params.length === 0)
            return false;

        return nodeHelpers.getUseNewlinesForNodes([getOpenParenToken(), params[0]]);

        function getOpenParenToken() {
            const paramHasParen = nodeHelpers.hasParentheses(params[0]);
            const firstOpenParen = tokenHelpers.getFirstOpenParenTokenBefore(params[0], context)!;

            return paramHasParen ? tokenHelpers.getFirstOpenParenTokenBefore(firstOpenParen, context) : firstOpenParen;
        }
    }
}

interface ParseFunctionOrMethodReturnTypeWithCloseParenOptions {
    context: Context;
    startInfo: Info;
    typeNode: babel.Node | null;
    typeNodeSeparator?: PrintItemIterable;
}

function* parseCloseParenWithType(opts: ParseFunctionOrMethodReturnTypeWithCloseParenOptions): PrintItemIterable {
    const { context, startInfo, typeNode, typeNodeSeparator } = opts;
    const returnTypeStartInfo = createInfo("returnTypeStart");
    const returnTypeEndInfo = createInfo("returnTypeEnd");
    // this is used in the true and false condition, so make it repeatable
    const parsedReturnTypeIterator = makeIterableRepeatable(parseReturnType());

    yield {
        kind: PrintItemKind.Condition,
        name: "newlineIfHeaderHangingAndReturnTypeMultipleLines",
        condition: conditionContext => {
            return conditionResolvers.isHanging(conditionContext, startInfo)
                && conditionResolvers.isMultipleLines(conditionContext, returnTypeStartInfo, returnTypeEndInfo);
        },
        true: function*() {
            yield context.newlineKind;
            yield ")";
            yield* parsedReturnTypeIterator;
        }(),
        false: function*() {
            if (typeNode)
                yield Signal.NewLine;

            yield ")";
            yield* parsedReturnTypeIterator;
        }()
    };

    function* parseReturnType(): PrintItemIterable {
        if (!typeNode)
            return;

        yield returnTypeStartInfo;

        if (typeNodeSeparator)
            yield* typeNodeSeparator;
        else
            yield ": ";

        yield* parseNode(typeNode, context);
        yield returnTypeEndInfo;
    }
}

function* parseNodeInParens(node: babel.Node, context: Context): PrintItemIterable {
    const openParenToken = tokenHelpers.getFirstOpenParenTokenBefore(node, context)!;
    const useNewLines = nodeHelpers.getUseNewlinesForNodes([openParenToken, node]);

    if (useNewLines)
        putDisableIndentInBagIfNecessaryForNode(node, context);

    const nodeIterator = parseNode(node, context);

    yield* parseIteratorInParens(nodeIterator, useNewLines, context);
}

function* parseIteratorInParens(iterator: PrintItemIterable, useNewLines: boolean, context: Context) {
    yield Signal.StartNewlineGroup;
    yield "(";

    if (useNewLines) {
        yield context.newlineKind;
        yield* withIndent(iterator);
        yield context.newlineKind;
    }
    else {
        yield* iterator;
    }

    yield ")";
    yield Signal.FinishNewLineGroup;
}

function* parseNamedImportsOrExports(
    parentDeclaration: babel.Node,
    namedImportsOrExports: (babel.ImportSpecifier | babel.ExportSpecifier)[],
    context: Context
): PrintItemIterable {
    if (namedImportsOrExports.length === 0)
        return;

    const useNewLines = getUseNewLines();
    const braceSeparator = useNewLines ? context.newlineKind : " ";

    yield "{";
    yield braceSeparator;

    if (useNewLines)
        yield* withIndent(newlineGroup(parseSpecifiers()));
    else
        yield* newlineGroup(parseSpecifiers());

    yield braceSeparator;
    yield "}";

    function getUseNewLines() {
        if (namedImportsOrExports.length === 0)
            return false;

        return nodeHelpers.getUseNewlinesForNodes([
            tokenHelpers.getFirstOpenBraceTokenWithin(parentDeclaration, context),
            namedImportsOrExports[0]
        ]);
    }

    function* parseSpecifiers(): PrintItemIterable {
        for (let i = 0; i < namedImportsOrExports.length; i++) {
            if (i > 0) {
                yield ",";
                yield useNewLines ? context.newlineKind : Signal.SpaceOrNewLine;
            }

            if (useNewLines)
                yield* parseNode(namedImportsOrExports[i], context);
            else
                yield* conditions.indentIfStartOfLine(parseNode(namedImportsOrExports[i], context));
        }
    }
}

/* helpers */

function* parseDecoratorsIfClass(declaration: babel.Node | undefined | null, context: Context): PrintItemIterable {
    if (declaration == null || declaration.type !== "ClassDeclaration" && declaration.type !== "ClassExpression")
        return;

    yield* parseDecorators(declaration, context);
}

function* parseDecorators(
    // explicitly type each member because the not smart code analysis will falsely pick up stuff
    // if using an intersection type here (ex. Node & { decorators: ...etc... })
    node: babel.ClassDeclaration | babel.ClassExpression | babel.ClassProperty | babel.ClassMethod | babel.TSDeclareMethod,
    context: Context
): PrintItemIterable {
    const decorators = node.decorators;
    if (decorators == null || decorators.length === 0)
        return;

    const isClassExpression = node.type === "ClassExpression";
    const useNewlines = isClassExpression ? false : nodeHelpers.getUseNewlinesForNodes(decorators);

    for (let i = 0; i < decorators.length; i++) {
        if (i > 0) {
            if (useNewlines)
                yield context.newlineKind;
            else
                yield Signal.SpaceOrNewLine;
        }

        if (isClassExpression)
            yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(decorators[i], context)));
        else
            yield* newlineGroup(parseNode(decorators[i], context));
    }

    if (isClassExpression)
        yield Signal.SpaceOrNewLine;
    else
        yield context.newlineKind;
}

function* parseForMemberLikeExpression(leftNode: babel.Node, rightNode: babel.Node, isComputed: boolean, context: Context): PrintItemIterable {
    const useNewline = nodeHelpers.getUseNewlinesForNodes([leftNode, rightNode]);

    yield* newlineGroup(function*() {
        yield* parseNode(leftNode, context);

        if (useNewline)
            yield context.newlineKind;
        else
            yield Signal.NewLine;

        yield* conditions.indentIfStartOfLine(parseRightNode());
    }());

    function* parseRightNode(): PrintItemIterable {
        if (isComputed)
            yield "[";
        else
            yield ".";

        yield* parseNode(rightNode, context);

        if (isComputed)
            yield "]";
    }
}

interface ParseExtendsOrImplementsOptions {
    text: "extends" | "implements";
    items: babel.Node[] | null | undefined;
    startHeaderInfo: Info;
    context: Context;
}

function* parseExtendsOrImplements(opts: ParseExtendsOrImplementsOptions) {
    const { text, items, context, startHeaderInfo } = opts;
    if (!items || items.length === 0)
        return;

    yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo);
    yield* conditions.indentIfStartOfLine(function*() {
        yield `${text} `;
        yield* newlineGroup(function*() {
            for (let i = 0; i < items.length; i++) {
                if (i > 0) {
                    yield ",";
                    yield Signal.SpaceOrNewLine;
                }

                yield* conditions.indentIfStartOfLine(parseNode(items[i], context));
            }
        }());
    }());
}

interface ParseArrayLikeNodesOptions {
    node: babel.Node;
    elements: ReadonlyArray<babel.Node | null | undefined>;
    trailingCommas: NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    context: Context;
}

function* parseArrayLikeNodes(opts: ParseArrayLikeNodesOptions) {
    const { node, elements, context } = opts;
    const useNewlines = nodeHelpers.getUseNewlinesForNodes(
        elements
            ? [tokenHelpers.getFirstOpenBracketTokenWithin(node, context), elements[0]]
            : []
    );
    const forceTrailingCommas = getForceTrailingCommas(opts.trailingCommas, useNewlines);

    yield "[";

    if (elements.length > 0)
        yield* parseElements();

    yield "]";

    function* parseElements(): PrintItemIterable {
        if (useNewlines)
            yield context.newlineKind;

        for (let i = 0; i < elements.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            const element = elements[i];
            const hasComma = forceTrailingCommas || i < elements.length - 1;
            yield* conditions.indentIfStartOfLine(parseElement(element, hasComma));

            if (useNewlines)
                yield context.newlineKind;
        }

        function* parseElement(element: babel.Node | null | undefined, hasComma: boolean): PrintItemIterable {
            if (element) {
                yield* parseNode(element, context, {
                    innerParse: function*(iterator) {
                        yield* iterator;

                        if (hasComma)
                            yield ",";
                    }
                });
            }
            else {
                if (hasComma)
                    yield ",";
            }
        }
    }
}

interface ParseObjectLikeNodeOptions {
    node: babel.Node;
    members: babel.Node[];
    context: Context;
    trailingCommas?: TypeScriptConfiguration["trailingCommas"];
}

function* parseObjectLikeNode(opts: ParseObjectLikeNodeOptions) {
    const { node, members, context, trailingCommas } = opts;

    if (members.length === 0) {
        yield "{}";
        return;
    }

    const multiLine = nodeHelpers.getUseNewlinesForNodes([tokenHelpers.getFirstOpenBraceTokenWithin(node, context), members[0]]);
    const startInfo = createInfo("startObject");
    const endInfo = createInfo("endObject");

    yield startInfo;
    yield "{";
    yield* getInner();
    yield getSeparator();
    yield "}";
    yield endInfo;

    function* getInner(): PrintItemIterable {
        yield getSeparator();

        if (multiLine) {
            yield* withIndent(parseStatementOrMembers({
                context,
                innerComments: node.innerComments,
                items: members,
                lastNode: undefined,
                shouldUseBlankLine: (previousStatement, nextStatement) => {
                    return nodeHelpers.hasSeparatingBlankLine(previousStatement, nextStatement);
                },
                trailingCommas
            }));
        }
        else {
            for (let i = 0; i < members.length; i++) {
                if (i > 0)
                    yield Signal.SpaceOrNewLine;

                yield* conditions.indentIfStartOfLine(parseNode(members[i], context, {
                    innerParse: function*(iterator) {
                        yield* iterator;

                        if (trailingCommas) {
                            const forceTrailingCommas = getForceTrailingCommas(trailingCommas, multiLine);
                            if (forceTrailingCommas || i < members.length - 1)
                                yield ",";
                        }
                    }
                }));
            }
        }
    }

    function getSeparator() {
        if (multiLine)
            return context.newlineKind;
        else
            return Signal.SpaceOrNewLine;
    }
}

function* getWithComments(node: babel.Node, printItemIterator: PrintItemIterable, context: Context): PrintItemIterable {
    yield* parseLeadingComments(node, context);
    yield* printItemIterator;
    yield* parseTrailingComments(node, context);
}

function* parseLeadingComments(node: babel.Node, context: Context) {
    if (!node.leadingComments)
        return;
    const lastComment = node.leadingComments[node.leadingComments.length - 1];
    const hasHandled = lastComment == null || context.handledComments.has(lastComment);

    yield* parseCommentCollection(node.leadingComments, undefined, context);

    if (lastComment != null && !hasHandled) {
        if (node.loc!.start.line > lastComment.loc!.end.line) {
            yield context.newlineKind;

            if (node.loc!.start.line - 1 > lastComment.loc!.end.line)
                yield context.newlineKind;
        }
        else if (lastComment.type === "CommentBlock" && lastComment.loc!.end.line === node.loc!.start.line) {
            yield " ";
        }
    }
}

function* parseTrailingComments(node: babel.Node, context: Context) {
    const trailingComments = getTrailingComments();
    if (!trailingComments)
        return;

    // use the roslyn definition of trailing comments
    const trailingCommentsOnSameLine = trailingComments.filter(c => c.loc!.start.line === node.loc!.end.line);
    if (trailingCommentsOnSameLine.length === 0)
        return;

    // add a space between the node and comment block since they'll be on the same line
    const firstUnhandledComment = trailingCommentsOnSameLine.find(c => !context.handledComments.has(c));
    if (firstUnhandledComment != null && firstUnhandledComment.type === "CommentBlock")
        yield " ";

    yield* parseCommentCollection(trailingCommentsOnSameLine, node, context);

    function getTrailingComments() {
        // These will not have trailing comments for comments that appear after a comma
        // so force them to appear.
        switch (context.parent.type) {
            case "ObjectExpression":
                return getTrailingCommentsWithNextLeading(context.parent.properties);
            case "ArrayExpression":
                return getTrailingCommentsWithNextLeading(context.parent.elements);
            case "TSTupleType":
                return getTrailingCommentsWithNextLeading(context.parent.elementTypes);
            default:
                return node.trailingComments;
        }

        function getTrailingCommentsWithNextLeading(nodes: (babel.Node | null)[]) {
            // todo: something faster than O(n)
            const index = nodes.indexOf(node);
            const nextProperty = nodes[index + 1];
            if (nextProperty) {
                return [
                    ...node.trailingComments || [],
                    ...nextProperty.leadingComments || []
                ];
            }
            return node.trailingComments;
        }
    }
}

function* parseCommentCollection(comments: Iterable<babel.Comment>, lastNode: (babel.Node | babel.Comment | undefined), context: Context) {
    for (const comment of comments) {
        if (context.handledComments.has(comment))
            continue;

        if (lastNode != null) {
            if (comment.loc.start.line > lastNode.loc!.end.line) {
                yield context.newlineKind;

                if (comment.loc.start.line > lastNode.loc!.end.line + 1)
                    yield context.newlineKind;
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

function* parseComment(comment: babel.Comment, context: Context): PrintItemIterable {
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

    function* parseCommentBlock(comment: babel.CommentBlock): PrintItemIterable {
        yield "/*";
        yield {
            kind: PrintItemKind.RawString,
            text: comment.value
        };
        yield "*/";
    }

    function* parseCommentLine(comment: babel.CommentLine): PrintItemIterable {
        yield parserHelpers.parseJsLikeCommentLine(comment.value);
        yield Signal.ExpectNewLine;
    }
}

function* parseFirstLineTrailingComments(node: babel.Node, members: babel.Node[], context: Context): PrintItemIterable {
    for (const trailingComment of getComments()) {
        if (trailingComment.loc!.start.line === node.loc!.start.line) {
            if (trailingComment.type === "CommentLine")
                yield " ";
            yield* parseComment(trailingComment, context);
        }
    }

    function* getComments() {
        if (node.innerComments)
            yield* node.innerComments;
        if (members.length > 0 && members[0].leadingComments)
            yield* members[0].leadingComments!;
        if (node.trailingComments)
            yield* node.trailingComments;
    }
}

interface ParseBraceSeparatorOptions {
    bracePosition: NonNullable<TypeScriptConfiguration["bracePosition"]>;
    bodyNode: babel.Node | nodeHelpers.BabelToken;
    startHeaderInfo: Info | undefined;
    context: Context;
}

function* parseBraceSeparator(opts: ParseBraceSeparatorOptions) {
    const { bracePosition, bodyNode, startHeaderInfo, context } = opts;

    if (bracePosition === "nextLineIfHanging") {
        if (startHeaderInfo == null)
            yield " ";
        else
            yield conditions.newlineIfHangingSpaceOtherwise(context, startHeaderInfo);
    }
    else if (bracePosition === "sameLine")
        yield " ";
    else if (bracePosition === "nextLine")
        yield context.newlineKind;
    else if (bracePosition === "maintain") {
        if (nodeHelpers.isFirstNodeOnLine(bodyNode, context))
            yield context.newlineKind;
        else
            yield " ";
    }
    else {
        assertNever(bracePosition);
    }
}

function* parseControlFlowSeparator(
    nextControlFlowPosition: NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>,
    nodeBlock: babel.Node,
    tokenText: string,
    context: Context
): PrintItemIterable {
    if (nextControlFlowPosition === "sameLine")
        yield " ";
    else if (nextControlFlowPosition === "nextLine")
        yield context.newlineKind;
    else if (nextControlFlowPosition === "maintain") {
        const token = getFirstControlFlowToken();
        if (token != null && nodeHelpers.isFirstNodeOnLine(token, context))
            yield context.newlineKind;
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
function* parseTypeAnnotationWithColonIfExists(node: babel.Node | null | undefined, context: Context) {
    yield* parseNodeWithPreceedingColon(node, context);
}

function* parseNodeWithPreceedingColon(node: babel.Node | null | undefined, context: Context) {
    if (node == null)
        return;

    yield ":";
    yield* newlineGroup(function*() {
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(parseNode(node, context));
    }());
}

function getForceTrailingCommas(option: NonNullable<TypeScriptConfiguration["trailingCommas"]>, useNewlines: boolean) {
    // this is explicit so that this is re-evaluated when the options change
    switch (option) {
        case "always":
            return true;
        case "onlyMultiLine":
            return useNewlines;
        case "never":
            return false;
        default:
            const assertNever: never = option;
            return false;
    }
}

function putDisableIndentInBagIfNecessaryForNode(node: babel.Node, context: Context) {
    if (node.type !== "LogicalExpression" && node.type !== "BinaryExpression")
        return;

    context.bag.put(BAG_KEYS.DisableIndentBool, true);
}

/* factory functions */

function createInfo(name: string): Info {
    return {
        kind: PrintItemKind.Info,
        name
    };
}
