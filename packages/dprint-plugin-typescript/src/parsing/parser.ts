import * as babel from "@babel/types";
import { makeIterableRepeatable, PrintItemKind, Signal, RawString, PrintItemIterable, Condition, Info, parserHelpers, conditions, conditionResolvers,
    resolveNewLineKindFromText, LoggingEnvironment, ResolveConditionContext, ResolveCondition } from "@dprint/core";
import { ResolvedTypeScriptConfiguration, TypeScriptConfiguration } from "../configuration";
import { assertNever, Bag, Stack, isStringEmptyOrWhiteSpace, hasNewlineOccurrencesInLeadingWhitespace, hasNewLineOccurrencesInTrailingWhitespace, throwError,
    hasNoNewlinesInLeadingWhitespace, hasNoNewlinesInTrailingWhitespace } from "../utils";
import { BabelToken } from "./BabelToken";
import * as nodeHelpers from "./nodeHelpers";
import * as tokenHelpers from "./tokenHelpers";
import { TokenFinder, isPrefixSemiColonInsertionChar, findNodeIndexInSortedArrayFast } from "./utils";

const { withIndent, newlineGroup, prependToIterableIfHasItems, toPrintItemIterable, surroundWithNewLines, createInfo } = parserHelpers;

/** Use this type to mark properties as ignored for the implemented-nodes.md report. */
type AnalysisMarkIgnored<T, Reason extends string> = T | Reason;
/** Use this type to mark properties as implemented for implemented-nodes.md report. */
type AnalysisMarkImplemented<T, Reason extends string> = T | Reason;

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
    bag: Bag;
    topBinaryOrLogicalExpressionInfos: Map<babel.BinaryExpression | babel.LogicalExpression, Info | false>;
    endStatementOrMemberInfo: Stack<Info>;
    tokenFinder: TokenFinder;
}

export interface ParseTypeScriptFileOptions {
    file: babel.File;
    filePath: string;
    fileText: string;
    config: ResolvedTypeScriptConfiguration;
    environment: LoggingEnvironment;
}

export function parseTypeScriptFile(options: ParseTypeScriptFileOptions): PrintItemIterable | false {
    type _markCommentsUsed = AnalysisMarkImplemented<typeof file.comments, "Comments are accessed in other ways.">;

    const { file, filePath, fileText, config, environment } = options;
    const context: Context = {
        file,
        fileText,
        log: message => environment.log(`${message} (${filePath})`),
        warn: message => environment.warn(`${message} (${filePath})`),
        config,
        handledComments: new Set<babel.Comment>(),
        currentNode: file,
        parentStack: [],
        parent: file,
        bag: new Bag(),
        topBinaryOrLogicalExpressionInfos: new Map<babel.BinaryExpression | babel.LogicalExpression, Info | false>(),
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
            true: [Signal.NewLine]
        };
    }();

    function shouldParseFile() {
        for (const comment of getCommentsToCheck()) {
            if (/\bdprint\-ignore\-file\b/.test(comment.value))
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
    "V8IntrinsicIdentifier": parseV8IntrinsicIdentifier,
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
    "OptionalMemberExpression": parseMemberExpression,
    "MetaProperty": parseMetaProperty,
    "NewExpression": parseNewExpression,
    "TSNonNullExpression": parseNonNullExpression,
    "ObjectExpression": parseObjectExpression,
    "ObjectMethod": parseClassOrObjectMethod,
    "ObjectPattern": parseObjectPattern,
    "ObjectProperty": parseObjectProperty,
    "RestElement": parseRestElement,
    "SequenceExpression": parseSequenceExpression,
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
    "TSBigIntKeyword": () => toPrintItemIterable("bigint"),
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
    /* jsx */
    "JSXAttribute": parseJsxAttribute,
    "JSXElement": parseJsxElement,
    "JSXEmptyExpression": parseJsxEmptyExpression,
    "JSXExpressionContainer": parseJsxExpressionContainer,
    "JSXOpeningElement": parseJsxOpeningElement,
    "JSXClosingElement": parseJsxClosingElement,
    "JSXFragment": parseJsxFragment,
    "JSXOpeningFragment": parseJsxOpeningFragment,
    "JSXClosingFragment": parseJsxClosingFragment,
    "JSXIdentifier": parseJsxIdentifier,
    "JSXMemberExpression": parseJsxMemberExpression,
    "JSXNamespacedName": parseJsxNamespacedName,
    "JSXSpreadAttribute": parseJsxSpreadAttribute,
    "JSXSpreadChild": parseJsxSpreadChild,
    "JSXText": parseJsxText,
    /* explicitly not implemented (most are proposals that haven't made it far enough) */
    "ArgumentPlaceholder": parseUnknownNode,
    "BindExpression": parseUnknownNode,
    "ClassPrivateMethod": parseUnknownNode,
    "ClassPrivateProperty": parseUnknownNode,
    "DoExpression": parseUnknownNode,
    "Noop": parseUnknownNode,
    "ParenthesizedExpression": parseUnknownNode, // this is disabled via createParenthesizedExpressions: false
    "PrivateName": parseUnknownNode,
    "PipelineBareFunction": parseUnknownNode,
    "PipelineTopicExpression": parseUnknownNode,
    "PipelinePrimaryTopicReference": parseUnknownNode,
    "Placeholder": parseUnknownNode,
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
    const printItemIterator = opts && opts.innerParse ? opts.innerParse(parseNode()) : parseNode();

    yield* getWithComments(node, printItemIterator, context);

    // replace the past info after iterating
    context.currentNode = context.parentStack.pop()!;
    context.parent = context.parentStack[context.parentStack.length - 1];

    function parseNode() {
        const nodeIterator = getNodeIterator();
        return nodeHelpers.hasParentheses(node!) ? parseInParens(nodeIterator) : nodeIterator;

        function getNodeIterator() {
            if (node && hasIgnoreComment())
                return toPrintItemIterable(parseNodeAsRawString(node, context));

            const parseFunc = parseObj[node!.type] || parseUnknownNode;
            return parseFunc(node, context);
        }
    }

    function parseInParens(nodeIterator: PrintItemIterable) {
        const openParenToken = tokenHelpers.getFirstOpenParenTokenBefore(node!, context)!;
        const useNewLines = nodeHelpers.getUseNewlinesForNodes([openParenToken, node]);

        if (useNewLines)
            putDisableIndentInBagIfNecessaryForNode(node!, context);

        return conditions.withIndentIfStartOfLineIndented(parseIteratorInParens(nodeIterator, useNewLines, context));
    }

    function hasIgnoreComment() {
        if (!node)
            return false;

        if (context.parent.type === "JSXElement" || context.parent.type === "JSXFragment") {
            const previousExpressionContainer = getPreviousJsxExpressionContainer(context.parent);

            if (previousExpressionContainer && previousExpressionContainer.expression.innerComments)
                return previousExpressionContainer.expression.innerComments.some(isIgnoreComment);
            return false;
        }

        if (!node.leadingComments)
            return false;

        for (let i = node.leadingComments.length - 1; i >= 0; i--) {
            const comment = node.leadingComments[i];
            if (context.handledComments.has(comment))
                continue;

            return isIgnoreComment(comment);
        }

        return false;

        function isIgnoreComment(comment: babel.Comment) {
            return /\bdprint\-ignore\b/.test(comment.value);
        }

        function getPreviousJsxExpressionContainer(parent: babel.JSXElement | babel.JSXFragment) {
            const currentIndex = findNodeIndexInSortedArrayFast(parent.children, node!);
            for (let i = currentIndex - 1; i >= 0; i--) {
                const previousChild = parent.children[i];
                if (previousChild.type === "JSXExpressionContainer")
                    return previousChild;
                if (previousChild.type !== "JSXText")
                    return undefined;
                // check if it's all white space and if so, ignore it
                if (!/^\s*$/.test(previousChild.value))
                    return undefined;
            }

            return undefined;
        }
    }
}

/* file */
function* parseProgram(node: babel.Program, context: Context): PrintItemIterable {
    type _ignoreSourceType = AnalysisMarkIgnored<typeof node.sourceType, "Not useful.">;
    type _ignoreSourceFile = AnalysisMarkIgnored<typeof node.sourceFile, "Not useful.">;

    if (node.interpreter) {
        yield* parseNode(node.interpreter, context);
        yield Signal.NewLine;

        if (nodeHelpers.hasSeparatingBlankLine(node.interpreter, node.directives[0] || node.body[0]))
            yield Signal.NewLine;
    }

    yield* parseStatements(node, context);
}

/* common */

function* parseBlockStatement(node: babel.BlockStatement, context: Context): PrintItemIterable {
    const startStatementsInfo = createInfo("startStatementsInfo");
    const endStatementsInfo = createInfo("endStatementsInfo");

    yield "{";

    // todo: isn't this a bug? These should be considered inner comments and be reported to babel
    const innerTrailingComments = node.trailingComments && node.trailingComments.filter(c => c.end < node.end!);
    if (innerTrailingComments && innerTrailingComments.length > 0)
        node.innerComments = [...node.innerComments || [], ...innerTrailingComments];

    // Allow: const t = () => {}; and const t = function() {};
    const isArrowOrFunctionExpression = context.parent.type === "ArrowFunctionExpression" || context.parent.type === "FunctionExpression";
    if (isArrowOrFunctionExpression && node.loc!.start.line === node.loc!.end.line
        && node.body.length === 0 && !node.leadingComments && !node.innerComments)
    {
        yield "}";
        return;
    }

    yield* parseFirstLineTrailingComments(node, node.body, context);
    yield Signal.NewLine;
    yield startStatementsInfo;
    yield* withIndent(parseStatements(node, context));
    yield endStatementsInfo;
    yield {
        kind: PrintItemKind.Condition,
        name: "endStatementsNewLine",
        condition: conditionContext => {
            return !conditionResolvers.areInfoEqual(conditionContext, startStatementsInfo, endStatementsInfo, false);
        },
        true: [Signal.NewLine]
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
}

function* parseV8IntrinsicIdentifier(node: babel.V8IntrinsicIdentifier, context: Context): PrintItemIterable {
    yield `%${node.name}`;
}

/* declarations */

function* parseClassDeclarationOrExpression(node: babel.ClassDeclaration | babel.ClassExpression, context: Context): PrintItemIterable {
    type _ignoreMixins = AnalysisMarkIgnored<typeof node.mixins, "Probably flow... doesn't seem to be used.">;

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
                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise({
                    startInfo: startHeaderInfo
                });
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

    if (context.config["exportAllDeclaration.semiColon"])
        yield ";";
}

function* parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context): PrintItemIterable {
    type _ignoreExportKind = AnalysisMarkIgnored<typeof node.exportKind, "Flow?">;

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

    if (node.declaration == null && context.config["exportNamedDeclaration.semiColon"])
        yield ";";
}

function* parseExportDefaultDeclaration(node: babel.ExportDefaultDeclaration, context: Context): PrintItemIterable {
    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export default ";
    yield* parseNode(node.declaration, context);

    if (shouldUseSemiColon())
        yield ";";

    function shouldUseSemiColon() {
        if (!context.config["exportDefaultDeclaration.semiColon"])
            return false;

        switch (node.declaration.type) {
            case "ClassDeclaration":
            case "FunctionDeclaration":
                return false;
            default:
                return true;
        }
    }
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

        if (getUseSpaceBeforeParens())
            yield " ";

        yield* parseParametersOrArguments({
            nodes: node.params,
            context,
            forceMultiLineWhenMultipleLines: node.type === "FunctionExpression"
                ? context.config["functionExpression.forceMultiLineParameters"]
                : context.config["functionDeclaration.forceMultiLineParameters"],
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

    function getUseSpaceBeforeParens() {
        switch (node.type) {
            case "TSDeclareFunction":
            case "FunctionDeclaration":
                return context.config["functionDeclaration.spaceBeforeParentheses"];
            case "FunctionExpression":
                return context.config["functionExpression.spaceBeforeParentheses"];
            default:
                return assertNever(node);
        }
    }
}

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): PrintItemIterable {
    type _ignoreImportKind = AnalysisMarkIgnored<typeof node.importKind, "Flow?">;

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
        const keyword = context.tokenFinder.getFirstTokenWithin(node, token => {
            return token.value && (token.value === "namespace" || token.value === "module") || false;
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
    yield* parseNode(node.typeAnnotation, context);

    if (context.config["typeAlias.semiColon"])
        yield ";";
}

function* parseTypeParameterDeclaration(
    declaration: babel.TSTypeParameterDeclaration | babel.TSTypeParameterInstantiation | babel.TypeParameterInstantiation,
    context: Context
): PrintItemIterable {
    const useNewLines = getUseNewLines();
    yield* parseItems();

    function* parseItems(): PrintItemIterable {
        yield "<";

        if (useNewLines)
            yield* surroundWithNewLines(parseParameterList());
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
                    yield Signal.NewLine;
                else
                    yield Signal.SpaceOrNewLine;
            }

            yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(param, context, {
                innerParse: function*(iterator) {
                    yield* iterator;
                    if (i < params.length - 1)
                        yield ",";
                }
            })));
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

            yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(node.declarations[i], context)));
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
    if (node.type !== "ObjectMethod") {
        type _ignoreAccess = AnalysisMarkIgnored<typeof node.access, "Flow.">;
        yield* parseDecorators(node, context);
    }

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

    if (getUseSpaceBeforeParens())
        yield " ";

    yield* parseParametersOrArguments({
        nodes: node.params,
        context,
        forceMultiLineWhenMultipleLines: getForceMultiLineParameters(),
        customCloseParen: parseCloseParenWithType({
            context,
            startInfo: startHeaderInfo,
            typeNode: node.returnType
        })
    });

    if (node.type !== "TSDeclareMethod") {
        yield* parseBraceSeparator({
            bracePosition: getBracePosition(),
            bodyNode: node.body,
            startHeaderInfo: startHeaderInfo,
            context
        });
        yield* parseNode(node.body, context);
    }
    else if (getUseSemiColon()) {
        yield ";";
    }

    function getForceMultiLineParameters() {
        switch (node.kind) {
            case "constructor":
                return context.config["constructor.forceMultiLineParameters"];
            case "get":
                return context.config["getAccessor.forceMultiLineParameters"];
            case "set":
                return context.config["setAccessor.forceMultiLineParameters"];
            case "method":
                return context.config["method.forceMultiLineParameters"];
            default:
                return assertNever(node);
        }
    }

    function getUseSpaceBeforeParens() {
        switch (node.kind) {
            case "constructor":
                return context.config["constructor.spaceBeforeParentheses"];
            case "get":
                return context.config["getAccessor.spaceBeforeParentheses"];
            case "set":
                return context.config["setAccessor.spaceBeforeParentheses"];
            case "method":
                return context.config["method.spaceBeforeParentheses"];
            default:
                return assertNever(node);
        }
    }

    function getBracePosition() {
        switch (node.kind) {
            case "constructor":
                return context.config["constructor.bracePosition"];
            case "get":
                return context.config["getAccessor.bracePosition"];
            case "set":
                return context.config["setAccessor.bracePosition"];
            case "method":
                return context.config["method.bracePosition"];
            default:
                return assertNever(node);
        }
    }

    function getUseSemiColon() {
        switch (node.kind) {
            case "constructor":
                return context.config["constructor.semiColon"];
            case "get":
                return context.config["getAccessor.semiColon"];
            case "set":
                return context.config["setAccessor.semiColon"];
            case "method":
                return context.config["method.semiColon"];
            default:
                return assertNever(node);
        }
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
    yield* parseParametersOrArguments({
        nodes: node.parameters,
        context,
        forceMultiLineWhenMultipleLines: context.config["callSignature.forceMultiLineParameters"],
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
    if (context.config["constructSignature.spaceAfterNewKeyword"])
        yield " ";
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments({
        nodes: node.parameters,
        context,
        forceMultiLineWhenMultipleLines: context.config["constructSignature.forceMultiLineParameters"],
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
    yield* parseParametersOrArguments({
        nodes: node.parameters,
        context,
        forceMultiLineWhenMultipleLines: context.config["methodSignature.forceMultiLineParameters"],
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
    yield " while";
    if (context.config["doWhileStatement.spaceAfterWhileKeyword"])
        yield " ";
    yield* parseNodeInParens({
        firstInnerNode: node.test,
        innerIterable: parseNode(node.test, context),
        context
    });

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
    if (context.config["expressionStatement.semiColon"])
        yield* parseInner();
    else
        yield* parseForPrefixSemiColonInsertion();

    function* parseInner(): PrintItemIterable {
        yield* parseNode(node.expression, context);

        if (context.config["expressionStatement.semiColon"])
            yield ";";
    }

    function* parseForPrefixSemiColonInsertion(): PrintItemIterable {
        const parsedNode = makeIterableRepeatable(parseInner());
        if (checkIterable(parsedNode))
            yield ";";
        yield* parsedNode;

        function checkIterable(iterable: PrintItemIterable): boolean | undefined {
            for (const item of iterable) {
                if (typeof item === "string")
                    return checkString(item);
                else if (typeof item === "number")
                    continue;
                else if (item.kind === PrintItemKind.Condition) {
                    const result = checkCondition(item);
                    if (result != null)
                        return result;
                }
                else if (item.kind === PrintItemKind.RawString)
                    return checkString(item.text);
                else if (item.kind === PrintItemKind.Info)
                    continue;
                else
                    assertNever(item);
            }
            return undefined;
        }

        function checkString(item: string) {
            return isPrefixSemiColonInsertionChar(item[0]);
        }

        function checkCondition(condition: Condition) {
            // It's an assumption here that the true and false paths of the
            // condition will both contain the same the same text to look for.
            if (condition.true) {
                condition.true = makeIterableRepeatable(condition.true);
                const result = checkIterable(condition.true);
                if (result != null)
                    return result;
            }
            if (condition.false) {
                condition.false = makeIterableRepeatable(condition.false);
                const result = checkIterable(condition.false);
                if (result != null)
                    return result;
            }
            return undefined;
        }
    }
}

function* parseForInStatement(node: babel.ForInStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");
    yield startHeaderInfo;
    yield "for";
    if (context.config["forInStatement.spaceAfterForKeyword"])
        yield " ";
    yield* parseNodeInParens({
        firstInnerNode: node.left,
        innerIterable: parseInnerHeader(),
        context
    });
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forInStatement.useBraces"],
        bracePosition: context.config["forInStatement.bracePosition"],
        singleBodyPosition: context.config["forInStatement.singleBodyPosition"],
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
    yield "for";
    if (context.config["forOfStatement.spaceAfterForKeyword"])
        yield " ";
    if (node.await)
        yield "await ";
    yield* parseNodeInParens({
        firstInnerNode: node.left,
        innerIterable: parseInnerHeader(),
        context
    });
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forOfStatement.useBraces"],
        bracePosition: context.config["forOfStatement.bracePosition"],
        singleBodyPosition: context.config["forOfStatement.singleBodyPosition"],
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
    yield "for";
    if (context.config["forStatement.spaceAfterForKeyword"])
        yield " ";
    yield* parseNodeInParens({
        firstInnerNode: node.init || context.tokenFinder.getFirstTokenWithin(node, ";")!,
        innerIterable: parseInnerHeader(),
        context
    });
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["forStatement.useBraces"],
        bracePosition: context.config["forStatement.bracePosition"],
        singleBodyPosition: context.config["forStatement.singleBodyPosition"],
        requiresBracesCondition: undefined,
        startHeaderInfo,
        endHeaderInfo
    }).iterator;

    function* parseInnerHeader(): PrintItemIterable {
        const separatorAfterSemiColons = getSeparatorAfterSemiColons();
        yield* newlineGroup(function*() {
            yield* parseNode(node.init, context);
            if (!node.init || node.init.type !== "VariableDeclaration")
                yield ";";
        }());
        yield separatorAfterSemiColons;
        yield* conditions.indentIfStartOfLine(newlineGroup(function*() {
            yield* parseNode(node.test, context);
            yield ";";
        }()));
        yield separatorAfterSemiColons;
        yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(node.update, context)));

        function getSeparatorAfterSemiColons() {
            return context.config["forStatement.spaceAfterSemiColons"] ? Signal.SpaceOrNewLine : Signal.PossibleNewLine;
        }
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
        singleBodyPosition: context.config["ifStatement.singleBodyPosition"],
        requiresBracesCondition: context.bag.take(BAG_KEYS.IfStatementLastBraceCondition) as Condition | undefined
    });

    yield* result.iterator;

    if (node.alternate) {
        if (node.alternate.type === "IfStatement" && node.alternate.alternate == null)
            context.bag.put(BAG_KEYS.IfStatementLastBraceCondition, result.braceCondition);

        yield* parseControlFlowSeparator(context.config["ifStatement.nextControlFlowPosition"], node.alternate, "else", context);

        // parse the leading comments before the else keyword
        yield* parseLeadingComments(node.alternate, context);

        const startElseHeaderInfo = createInfo("startElseHeader");
        yield startElseHeaderInfo;
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
                startHeaderInfo: startElseHeaderInfo,
                useBraces: context.config["ifStatement.useBraces"],
                bracePosition: context.config["ifStatement.bracePosition"],
                singleBodyPosition: context.config["ifStatement.singleBodyPosition"],
                headerStartToken: context.tokenFinder.getFirstTokenBefore(node.alternate, "else"),
                requiresBracesCondition: result.braceCondition
            }).iterator;
        }
    }

    function* parseHeader(ifStatement: babel.IfStatement): PrintItemIterable {
        yield "if";
        if (context.config["ifStatement.spaceAfterIfKeyword"])
            yield " ";
        yield* parseNodeInParens({
            firstInnerNode: ifStatement.test,
            innerIterable: parseNode(ifStatement.test, context),
            context
        });
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
        yield Signal.NewLine;

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
    const startHeaderInfo = createInfo("switchCaseStartHeader");
    yield startHeaderInfo;

    if (node.test == null)
        yield "default:";
    else {
        yield "case ";
        yield* parseNode(node.test, context);
        yield ":";
    }

    yield* parseFirstLineTrailingComments(node, node.consequent, context);

    if (node.consequent.length > 0) {
        const blockStatementBody = getBlockStatementBody();

        if (blockStatementBody) {
            yield* parseBraceSeparator({
                bracePosition: context.config["switchCase.bracePosition"],
                bodyNode: blockStatementBody,
                startHeaderInfo: startHeaderInfo,
                context
            });
            yield* parseNode(blockStatementBody, context);
        }
        else {
            yield Signal.NewLine;
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

    yield* parseTrailingCommentStatements();

    function* parseTrailingCommentStatements(): PrintItemIterable {
        // Trailing comments in switch statements should use the child switch statement
        // indentation until a comment is reached that uses the case indentation level.
        // The last switch case's trailing comments should use the child indentation.
        const trailingComments = Array.from(getTrailingCommentsAsStatements(node, context));
        if (trailingComments.length === 0)
            return;

        const parentCases = (context.parent as babel.SwitchStatement).cases;
        const isLastSwitchCase = parentCases[parentCases.length - 1] === node;

        let isEqualIndent = getBlockStatementBody() != null;
        let lastNode: babel.Node | babel.Comment = node;

        for (const comment of trailingComments) {
            isEqualIndent = isEqualIndent || comment.loc!.start.column <= lastNode.loc!.start.column;
            const parsedCommentNode = parseCommentBasedOnLastNode(comment, lastNode, context);

            if (!isLastSwitchCase && isEqualIndent)
                yield* parsedCommentNode;
            else
                yield* withIndent(parsedCommentNode);

            lastNode = comment;
        }
    }

    function getBlockStatementBody() {
        if (node.consequent.length === 1 && node.consequent[0].type === "BlockStatement")
            return node.consequent[0] as babel.BlockStatement;
        return undefined;
    }
}

function* parseSwitchStatement(node: babel.SwitchStatement, context: Context): PrintItemIterable {
    const startHeaderInfo = createInfo("startHeader");
    yield startHeaderInfo;
    yield "switch ";
    yield* parseNodeInParens({
        firstInnerNode: node.discriminant,
        innerIterable: parseNode(node.discriminant, context),
        context
    });

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
    yield "while";
    if (context.config["whileStatement.spaceAfterWhileKeyword"])
        yield " ";
    yield* parseNodeInParens({
        firstInnerNode: node.test,
        innerIterable: parseNode(node.test, context),
        context
    });
    yield endHeaderInfo;

    yield* parseConditionalBraceBody({
        context,
        parent: node,
        bodyNode: node.body,
        useBraces: context.config["whileStatement.useBraces"],
        bracePosition: context.config["whileStatement.bracePosition"],
        singleBodyPosition: context.config["whileStatement.singleBodyPosition"],
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
    singleBodyPosition?: TypeScriptConfiguration["singleBodyPosition"];
}

interface ParseHeaderWithConditionalBraceBodyResult {
    iterator: PrintItemIterable;
    braceCondition: Condition;
}

function parseHeaderWithConditionalBraceBody(opts: ParseHeaderWithConditionalBraceBodyOptions): ParseHeaderWithConditionalBraceBodyResult {
    const { context, parent, bodyNode, requiresBracesCondition, useBraces, bracePosition, singleBodyPosition } = opts;
    const startHeaderInfo = createInfo("startHeader");
    const endHeaderInfo = createInfo("endHeader");

    const result = parseConditionalBraceBody({
        context,
        parent,
        bodyNode,
        requiresBracesCondition,
        useBraces,
        bracePosition,
        singleBodyPosition,
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
    singleBodyPosition?: TypeScriptConfiguration["singleBodyPosition"];
    requiresBracesCondition: Condition | undefined;
    headerStartToken?: BabelToken;
    startHeaderInfo?: Info;
    endHeaderInfo?: Info;
}

interface ParseConditionalBraceBodyResult {
    iterator: PrintItemIterable;
    braceCondition: Condition;
}

function parseConditionalBraceBody(opts: ParseConditionalBraceBodyOptions): ParseConditionalBraceBodyResult {
    const { startHeaderInfo, endHeaderInfo, parent, bodyNode, context, requiresBracesCondition, useBraces, bracePosition, singleBodyPosition,
        headerStartToken } = opts;
    const startStatementsInfo = createInfo("startStatements");
    const endStatementsInfo = createInfo("endStatements");
    const headerTrailingComments = Array.from(getHeaderTrailingComments());
    const openBraceCondition: Condition = {
        kind: PrintItemKind.Condition,
        name: "openBrace",
        condition: conditionContext => {
            if (useBraces === "whenNotSingleLine")
                return conditionContext.getResolvedCondition(newlineOrSpaceCondition);
            else if (useBraces === "maintain")
                return bodyNode.type === "BlockStatement";
            else if (useBraces === "always")
                return true;
            else if (useBraces === "preferNone") {
                // writing an open brace might make the header hang, so assume it should
                // not write the open brace until it's been resolved
                return bodyShouldBeMultiLine()
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
    const newlineOrSpaceCondition: Condition = {
        kind: PrintItemKind.Condition,
        name: "newlineOrSpaceCondition",
        condition: conditionContext => {
            if (shouldUseNewline())
                return true;
            if (startHeaderInfo == null)
                return throwError("Expected start header info in this scenario.");

            const resolvedStartInfo = conditionContext.getResolvedInfo(startHeaderInfo)!;
            if (resolvedStartInfo.lineNumber < conditionContext.writerInfo.lineNumber)
                return true;

            const resolvedEndStatementsInfo = conditionContext.getResolvedInfo(endStatementsInfo);
            if (resolvedEndStatementsInfo == null)
                return undefined;

            return resolvedEndStatementsInfo.lineNumber > resolvedStartInfo.lineNumber;

            function shouldUseNewline() {
                if (bodyShouldBeMultiLine())
                    return true;
                if (singleBodyPosition == null)
                    return true;
                switch (singleBodyPosition) {
                    case "maintain":
                        return getBodyStatementStartLine() > getHeaderStartLine();
                    case "nextLine":
                        return true;
                    case "sameLine":
                        if (bodyNode.type === "BlockStatement") {
                            if (bodyNode.body.length !== 1)
                                return true;
                            return getBodyStatementStartLine() > getHeaderStartLine();
                        }
                        return false;
                    default:
                        return assertNever(singleBodyPosition);
                }

                function getHeaderStartLine() {
                    return (headerStartToken || parent).loc!.start.line;
                }

                function getBodyStatementStartLine() {
                    if (bodyNode.type === "BlockStatement") {
                        const firstStatement = bodyNode.body[0];
                        if (firstStatement)
                            return firstStatement && firstStatement.loc!.start.line;
                    }
                    return bodyNode.loc!.start.line;
                }
            }
        },
        true: [Signal.NewLine],
        false: [" "]
    };

    return {
        braceCondition: openBraceCondition,
        iterator: parseBody()
    };

    function* parseBody(): PrintItemIterable {
        yield openBraceCondition;

        yield* parseHeaderTrailingComment();

        yield newlineOrSpaceCondition;

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
                    if (!conditionContext.getResolvedCondition(newlineOrSpaceCondition))
                        return false;
                    return !conditionResolvers.areInfoEqual(conditionContext, startStatementsInfo, endStatementsInfo, false);
                },
                true: [Signal.NewLine],
                false: [{
                    kind: PrintItemKind.Condition,
                    name: "closeBraceSpace",
                    condition: conditionContext => {
                        return !conditionContext.getResolvedCondition(newlineOrSpaceCondition);
                    },
                    true: " "
                }]
            }, "}"]
        };

        function* parseHeaderTrailingComment(): PrintItemIterable {
            const result = parseCommentCollection(headerTrailingComments, undefined, context);
            yield* prependToIterableIfHasItems(result, " "); // add a space
        }
    }

    function bodyShouldBeMultiLine() {
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
    type _ignoreExpression = AnalysisMarkIgnored<typeof node.expression, "Don't care about this boolean because the body contains this info.">;
    type _ignoreGenerator = AnalysisMarkIgnored<typeof node.generator, "Arrow function expressions can't be generators.">;

    const headerStartInfo = createInfo("functionExpressionHeaderStart");
    yield headerStartInfo;

    if (node.async)
        yield "async ";

    yield* parseNode(node.typeParameters, context);

    if (shouldUseParens()) {
        yield* parseParametersOrArguments({
            nodes: node.params,
            context,
            forceMultiLineWhenMultipleLines: context.config["arrowFunctionExpression.forceMultiLineParameters"],
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

    function shouldUseParens() {
        const firstParam = node.params[0];
        const requiresParens = node.params.length !== 1 || node.returnType || firstParam.type !== "Identifier" || firstParam.typeAnnotation != null;
        if (requiresParens)
            return true;
        const configSetting = context.config["arrowFunctionExpression.useParentheses"];
        if (configSetting === "force")
            return true;
        else if (configSetting === "maintain")
            return hasParentheses();
        else if (configSetting === "preferNone")
            return false;
        else
            return assertNever(configSetting);
    }

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
    type _ignoreTypeAnnotation = AnalysisMarkIgnored<typeof node.typeAnnotation, "Flow.">;

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
    const useSpaceSurroundingOperator = getUseSpaceSurroundingOperator();
    const topMostExpr = getTopMostBinaryOrLogicalExpression();
    const isTopMost = topMostExpr === node;
    const topMostInfo = getOrSetTopMostInfo();

    if (isTopMost && topMostInfo !== false)
        yield topMostInfo;

    yield* isExpressionBreakable(node) ? innerParse() : newlineGroup(innerParse());

    function* innerParse(): PrintItemIterable {
        const operatorPosition = getOperatorPosition();
        const useNewLines = getUseNewLines();

        yield indentIfNecessary(node.left, newlineGroupIfNecessary(node.left, parseNode(node.left, context, {
            innerParse: function*(iterable) {
                yield* iterable;
                if (operatorPosition === "sameLine") {
                    if (useSpaceSurroundingOperator)
                        yield " ";
                    yield node.operator;
                }
            }
        })));

        yield* parseCommentsAsTrailing(node.left, node.right.leadingComments, context);

        if (useNewLines)
            yield Signal.NewLine;
        else if (useSpaceSurroundingOperator)
            yield Signal.SpaceOrNewLine;
        else
            yield Signal.PossibleNewLine;

        yield indentIfNecessary(node.right, function*() {
            yield* parseCommentsAsLeading(node, node.left.trailingComments, context);
            yield* parseNode(node.right, context, {
                innerParse: function*(iterable) {
                    if (operatorPosition === "nextLine") {
                        yield node.operator;
                        if (useSpaceSurroundingOperator)
                            yield " ";
                    }
                    yield* newlineGroupIfNecessary(node.right, iterable);
                }
            });
        }());

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

        function* newlineGroupIfNecessary(expression: babel.Node, iterable: PrintItemIterable): PrintItemIterable {
            if (!isBinaryOrLogicalExpression(expression))
                yield* newlineGroup(iterable);
            else
                yield* iterable;
        }

        function getOperatorPosition() {
            const configValue = getConfigValue();

            switch (configValue) {
                case "nextLine":
                case "sameLine":
                    return configValue;
                case "maintain":
                    const operatorToken = context.tokenFinder.getFirstTokenAfter(node.left, node.operator)!;
                    return node.left.loc!.end.line === operatorToken.loc!.start.line ? "sameLine" : "nextLine";
                default:
                    return assertNever(configValue);
            }

            function getConfigValue() {
                switch (node.type) {
                    case "BinaryExpression":
                        return context.config["binaryExpression.operatorPosition"];
                    case "LogicalExpression":
                        return context.config["logicalExpression.operatorPosition"];
                    default:
                        return assertNever(node);
                }
            }
        }
    }

    function indentIfNecessary(currentNode: babel.Node, iterable: PrintItemIterable): Condition {
        iterable = makeIterableRepeatable(iterable);
        return {
            kind: PrintItemKind.Condition,
            name: "indentIfNecessaryForBinaryAndLogicalExpressions",
            condition: conditionContext => {
                // do not indent if indenting is disabled
                if (topMostInfo === false)
                    return false;
                // do not indent if this is the left most node
                if (nodeHelpers.getStartOrParenStart(topMostExpr) === nodeHelpers.getStartOrParenStart(currentNode))
                    return false;

                const resolvedTopMostInfo = conditionContext.getResolvedInfo(topMostInfo)!;
                const isSameIndent = resolvedTopMostInfo.indentLevel === conditionContext.writerInfo.indentLevel;
                return isSameIndent && conditionResolvers.isStartOfNewLine(conditionContext);
            },
            true: withIndent(iterable),
            false: iterable
        };
    }

    function getTopMostBinaryOrLogicalExpression() {
        let topMost = node;
        for (let i = context.parentStack.length - 1; i >= 0; i--) {
            if (nodeHelpers.hasParentheses(topMost))
                break;
            const ancestor = context.parentStack[i];
            if (!isBinaryOrLogicalExpression(ancestor))
                break;
            topMost = ancestor;
        }
        return topMost;
    }

    function getOrSetTopMostInfo() {
        if (isTopMost) {
            const allowIndent = context.bag.take(BAG_KEYS.DisableIndentBool) == null;
            const info = allowIndent ? createInfo("topBinaryOrLogicalExpressionStart") : false;
            context.topBinaryOrLogicalExpressionInfos.set(topMostExpr, info);
            return info;
        }
        else {
            return context.topBinaryOrLogicalExpressionInfos.get(topMostExpr)!;
        }
    }

    function isBinaryOrLogicalExpression(node: babel.Node): node is babel.BinaryExpression | babel.LogicalExpression {
        return node.type === "BinaryExpression" || node.type === "LogicalExpression";
    }

    function isExpressionBreakable(expr: babel.LogicalExpression | babel.BinaryExpression) {
        switch (expr.operator) {
            case "&&":
            case "||":
            case "+":
            case "-":
            case "*":
            case "/":
                return true;
            default:
                return false;
        }
    }

    function getUseSpaceSurroundingOperator() {
        switch (node.type) {
            case "BinaryExpression":
                return context.config["binaryExpression.spaceSurroundingOperator"];
            case "LogicalExpression":
                return true;
            default:
                return assertNever(node);
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

        yield* conditions.withIndentIfStartOfLineIndented(parseParametersOrArguments({
            nodes: node.arguments,
            context,
            forceMultiLineWhenMultipleLines: context.config["callExpression.forceMultiLineArguments"]
        }));
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

        /** Stop the iterator from providing any formatting information (ex. Signal.PossibleNewLine). */
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
    const operatorPosition = getOperatorPosition();
    const startInfo = createInfo("startConditionalExpression");
    const beforeAlternateInfo = createInfo("afterAlternateColon");
    const endInfo = createInfo("endConditionalExpression");

    yield startInfo;
    yield* newlineGroup(parseNode(node.test, context, {
        innerParse: function*(iterator) {
            yield* iterator;
            if (operatorPosition === "sameLine")
                yield " ?";
        }
    }));
    yield* parseConsequentAndAlternate();

    function* parseConsequentAndAlternate(): PrintItemIterable {
        // force re-evaluation of all the conditions below
        // once the endInfo has been reached
        yield conditions.forceReevaluationOnceResolved(context.endStatementOrMemberInfo.peek() || endInfo);

        if (useNewlines)
            yield Signal.NewLine;
        else {
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise({
                startInfo,
                endInfo: beforeAlternateInfo
            });
        }

        yield* conditions.indentIfStartOfLine(function*() {
            if (operatorPosition === "nextLine")
                yield "? ";
            yield* newlineGroup(parseNode(node.consequent, context, {
                innerParse: function*(iterator) {
                    yield* iterator;
                    if (operatorPosition === "sameLine")
                        yield " :";
                }
            }));
        }());

        if (useNewlines)
            yield Signal.NewLine;
        else {
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise({
                startInfo,
                endInfo: beforeAlternateInfo
            });
        }

        yield* conditions.indentIfStartOfLine(function*() {
            if (operatorPosition === "nextLine")
                yield ": ";
            yield beforeAlternateInfo;
            yield* newlineGroup(parseNode(node.alternate, context));
            yield endInfo;
        }());
    }

    function getOperatorPosition() {
        const configValue = context.config["conditionalExpression.operatorPosition"];
        switch (configValue) {
            case "nextLine":
            case "sameLine":
                return configValue;
            case "maintain":
                const operatorToken = context.tokenFinder.getFirstTokenAfter(node.test, "?")!;
                return node.test.loc!.end.line === operatorToken.loc!.start.line ? "sameLine" : "nextLine";
            default:
                return assertNever(configValue);
        }
    }
}

function* parseMemberExpression(node: babel.MemberExpression | babel.OptionalMemberExpression, context: Context): PrintItemIterable {
    yield* parseForMemberLikeExpression(node, node.object, node.property, node.computed, context);
}

function* parseMetaProperty(node: babel.MetaProperty, context: Context): PrintItemIterable {
    yield* parseForMemberLikeExpression(node, node.meta, node.property, false, context);
}

function* parseNewExpression(node: babel.NewExpression, context: Context): PrintItemIterable {
    type _ignoreTypeArguments = AnalysisMarkIgnored<typeof node.typeArguments, "Flow.">;

    yield "new ";
    yield* parseNode(node.callee, context);
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments({
        nodes: node.arguments,
        context,
        forceMultiLineWhenMultipleLines: context.config["newExpression.forceMultiLineArguments"]
    });
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

function* parseSequenceExpression(node: babel.SequenceExpression, context: Context): PrintItemIterable {
    yield* parseCommaSeparatedValues({
        values: node.expressions,
        context,
        multiLineOrHangingConditionResolver: () => false
    });
}

function* parseSpreadElement(node: babel.SpreadElement, context: Context): PrintItemIterable {
    yield "...";
    yield* parseNode(node.argument, context);
}

function* parseTaggedTemplateExpression(node: babel.TaggedTemplateExpression, context: Context): PrintItemIterable {
    yield* parseNode(node.tag, context);
    yield* parseNode(node.typeParameters, context);
    yield Signal.SpaceOrNewLine;
    yield* conditions.indentIfStartOfLine(parseNode(node.quasi, context));
}

function* parseTypeAssertion(node: babel.TSTypeAssertion, context: Context): PrintItemIterable {
    yield "<";
    yield* parseNode(node.typeAnnotation, context);
    yield ">";
    if (context.config["typeAssertion.spaceBeforeExpression"])
        yield " ";
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

function parseImportDefaultSpecifier(node: babel.ImportDefaultSpecifier, context: Context) {
    return parseNode(node.local, context);
}

function* parseImportNamespaceSpecifier(node: babel.ImportNamespaceSpecifier, context: Context): PrintItemIterable {
    yield "* as ";
    yield* parseNode(node.local, context);
}

function* parseImportSpecifier(node: babel.ImportSpecifier, context: Context): PrintItemIterable {
    type _ignoreImportKind = AnalysisMarkIgnored<typeof node.importKind, "Not sure what this is, but doesn't seem to be useful?">;

    if (node.imported.start === node.local.start) {
        yield* parseNode(node.imported, context);
        return;
    }

    yield* parseNode(node.imported, context);
    yield Signal.SpaceOrNewLine;
    yield* conditions.indentIfStartOfLine(function*() {
        yield "as ";
        yield* parseNode(node.local, context);
    }());
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

function* parseExportSpecifier(node: babel.ExportSpecifier, context: Context): PrintItemIterable {
    if (node.local.start === node.exported.start) {
        yield* parseNode(node.local, context);
        return;
    }

    yield* parseNode(node.local, context);
    yield Signal.SpaceOrNewLine;
    yield* conditions.indentIfStartOfLine(function*() {
        yield "as ";
        yield* parseNode(node.exported, context);
    }());
}

/* literals */

function* parseBigIntLiteral(node: babel.BigIntLiteral, context: Context): PrintItemIterable {
    yield node.value + "n";
}

function* parseBooleanLiteral(node: babel.BooleanLiteral, context: Context): PrintItemIterable {
    yield node.value ? "true" : "false";
}

function* parseNumericLiteral(node: babel.NumericLiteral, context: Context): PrintItemIterable {
    type _markValueUsed = AnalysisMarkImplemented<typeof node.value, "This is essentially used by accessing the text.">;

    yield context.fileText.substring(node.start!, node.end!);
}

function* parseStringOrDirectiveLiteral(node: babel.StringLiteral | babel.DirectiveLiteral, context: Context): PrintItemIterable {
    type _markValueUsed = AnalysisMarkImplemented<typeof node.value, "This is essentially used by accessing the text.">;

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
    type _markValueUsed = AnalysisMarkImplemented<typeof node.value, "Used by getting the text.">;
    type _markTailUsed = AnalysisMarkImplemented<typeof node.tail, "Not useful for our situation.">;

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
                yield Signal.PossibleNewLine;
                yield conditions.singleIndentIfStartOfLine();
                yield* parseNode(item, context);
                yield Signal.PossibleNewLine;
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
    const rawString = parseNodeAsRawString(node, context);

    context.log(`${message}: ${node.type} (${rawString.text.substring(0, 100)})`);

    return rawString;
}

function parseNodeAsRawString(node: babel.Node, context: Context): RawString {
    const nodeText = context.fileText.substring(node.start!, node.end!);

    return {
        kind: PrintItemKind.RawString,
        text: nodeText
    };
}

/* types */

function* parseArrayType(node: babel.TSArrayType, context: Context): PrintItemIterable {
    yield* parseNode(node.elementType, context);
    yield "[]";
}

function* parseConditionalType(node: babel.TSConditionalType, context: Context): PrintItemIterable {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes([node.checkType, node.falseType]);
    const isParentConditionalType = context.parent.type === "TSConditionalType";

    yield* parseMainArea();
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
            yield Signal.NewLine;
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
    if (context.config["constructorType.spaceAfterNewKeyword"])
        yield " ";
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments({
        nodes: node.parameters,
        context,
        forceMultiLineWhenMultipleLines: context.config["constructorType.forceMultiLineParameters"],
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
    const startInfo = createInfo("startFunctionType");
    yield startInfo;
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments({
        nodes: node.parameters,
        context,
        forceMultiLineWhenMultipleLines: context.config["functionType.forceMultiLineParameters"],
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

    yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise({
        startInfo
    });
    yield "}";

    function* parseLayout(): PrintItemIterable {
        if (useNewLines)
            yield Signal.NewLine;
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
    yield* conditions.withIndentIfStartOfLineIndented(parseNodeInParens({
        firstInnerNode: node.typeAnnotation,
        innerIterable: parseNode(node.typeAnnotation, context),
        context
    }));
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
            yield Signal.NewLine;

        for (let i = 0; i < node.elementTypes.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(node.elementTypes[i], context, {
                innerParse: function*(iterator) {
                    yield* iterator;

                    if (forceTrailingCommas || i < node.elementTypes.length - 1)
                        yield ",";
                }
            })));

            if (useNewlines)
                yield Signal.NewLine;
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
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            if (context.parent.type === "TSMappedType")
                yield "in ";
            else
                yield "extends ";

            yield* parseNode(node.constraint, context);
        }());
    }

    if (node.default) {
        yield Signal.SpaceOrNewLine;
        yield* conditions.indentIfStartOfLine(function*() {
            yield "= ";
            yield* parseNode(node.default, context);
        }());
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
    const isAncestorUnionOrIntersectionType = context.parent.type === "TSUnionType" || context.parent.type === "TSIntersectionType";

    for (let i = 0; i < node.types.length; i++) {
        if (i > 0)
            yield useNewLines ? Signal.NewLine : Signal.SpaceOrNewLine;

        // probably something better needs to be done here, but this is good enough for now
        if (isAncestorParenthesizedType || i == 0 && !isAncestorUnionOrIntersectionType)
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

/* jsx */

function* parseJsxAttribute(node: babel.JSXAttribute, context: Context): PrintItemIterable {
    yield* parseNode(node.name, context);
    yield "=";
    yield* parseNode(node.value, context);
}

function* parseJsxElement(node: babel.JSXElement, context: Context): PrintItemIterable {
    type _markSelfClosingUsed = AnalysisMarkImplemented<typeof node.selfClosing, "This is used by checking the closing element.">;

    if (node.closingElement == null)
        yield* parseNode(node.openingElement, context);
    else {
        yield* parseJsxWithOpeningAndClosing({
            node,
            children: node.children,
            openingElement: node.openingElement,
            closingElement: node.closingElement,
            context
        });
    }
}

function* parseJsxEmptyExpression(node: babel.JSXEmptyExpression, context: Context): PrintItemIterable {
    if (node.innerComments)
        yield* parseCommentCollection(node.innerComments, undefined, context);
}

function* parseJsxExpressionContainer(node: babel.JSXExpressionContainer, context: Context): PrintItemIterable {
    const surroundWithSpace = context.config["jsxExpressionContainer.spaceSurroundingExpression"];
    yield "{";
    if (surroundWithSpace)
        yield " ";
    yield* parseNode(node.expression, context);
    if (surroundWithSpace)
        yield " ";
    yield "}";
}

function* parseJsxOpeningElement(node: babel.JSXOpeningElement, context: Context): PrintItemIterable {
    const isMultiLine = getIsMultiLine();
    const startInfo = createInfo("openingElementStartInfo");

    yield startInfo;
    yield "<";
    yield* parseNode(node.name, context);
    yield* parseNode(node.typeParameters, context);
    yield* parseAttributes();
    if (node.selfClosing) {
        if (!isMultiLine)
            yield " ";
        yield "/";
    }
    else {
        yield {
            kind: PrintItemKind.Condition,
            name: "newlineIfHanging",
            condition: conditionContext => conditionResolvers.isHanging(conditionContext, startInfo),
            true: [Signal.NewLine]
        };
    }
    yield ">";

    function* parseAttributes(): PrintItemIterable {
        if (node.attributes.length === 0)
            return;

        for (const attrib of node.attributes)
            yield* parseAttrib(attrib);

        if (isMultiLine)
            yield Signal.NewLine;

        function* parseAttrib(attrib: babel.Node): PrintItemIterable {
            if (isMultiLine)
                yield Signal.NewLine;
            else
                yield Signal.SpaceOrNewLine;

            yield* conditions.indentIfStartOfLine(parseNode(attrib, context));
        }
    }

    function getIsMultiLine() {
        return nodeHelpers.getUseNewlinesForNodes([node.name, node.attributes[0]]);
    }
}

function* parseJsxClosingElement(node: babel.JSXClosingElement, context: Context): PrintItemIterable {
    yield "</";
    yield* parseNode(node.name, context);
    yield ">";
}

function* parseJsxFragment(node: babel.JSXFragment, context: Context): PrintItemIterable {
    yield* parseJsxWithOpeningAndClosing({
        node,
        children: node.children,
        openingElement: node.openingFragment,
        closingElement: node.closingFragment,
        context
    });
}

function* parseJsxOpeningFragment(node: babel.JSXOpeningFragment, context: Context): PrintItemIterable {
    yield "<>";
}

function* parseJsxClosingFragment(node: babel.JSXClosingFragment, context: Context): PrintItemIterable {
    yield "</>";
}

function* parseJsxIdentifier(node: babel.JSXIdentifier, context: Context): PrintItemIterable {
    yield node.name;
}

function* parseJsxMemberExpression(node: babel.JSXMemberExpression, context: Context): PrintItemIterable {
    yield* parseNode(node.object, context);
    yield ".";
    yield* parseNode(node.property, context);
}

function* parseJsxNamespacedName(node: babel.JSXNamespacedName, context: Context): PrintItemIterable {
    yield* parseNode(node.namespace, context);
    yield ":";
    yield* parseNode(node.name, context);
}

function* parseJsxSpreadAttribute(node: babel.JSXSpreadAttribute, context: Context): PrintItemIterable {
    yield "{...";
    yield* parseNode(node.argument, context);
    yield "}";
}

function* parseJsxSpreadChild(node: babel.JSXSpreadChild, context: Context): PrintItemIterable {
    yield "{...";
    yield* parseNode(node.expression, context);
    yield "}";
}

function* parseJsxText(node: babel.JSXText, context: Context): PrintItemIterable {
    type _markValueImplemented = AnalysisMarkImplemented<typeof node.value, "Implemented via extra.raw">;
    // todo: how expensive is trim()?
    const lines = nodeHelpers.getJsxText(node).trim().split(/\r?\n/g).map(line => line.trimRight());

    for (let i = 0; i < lines.length; i++) {
        const lineText = lines[i];
        if (i > 0) {
            if (lineText.length > 0 || i === 1 || lines[i - 1].length === 0 && lines[i - 2].length > 0)
                yield Signal.NewLine;
        }

        if (lineText.length > 0)
            yield lineText;
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
    yield Signal.NewLine;
    yield "}";

    function* parseBody(): PrintItemIterable {
        if (members.length > 0 || node.innerComments != null && node.innerComments.some(n => !context.handledComments.has(n)))
            yield Signal.NewLine;

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

interface ParseJsxWithOpeningAndClosingOptions {
    node: babel.Node;
    openingElement: babel.Node;
    closingElement: babel.Node;
    children: babel.Node[];
    context: Context;
}

function* parseJsxWithOpeningAndClosing(opts: ParseJsxWithOpeningAndClosingOptions): PrintItemIterable {
    const { node, children: allChildren, openingElement, closingElement, context } = opts;
    const children = allChildren.filter(c => c.type !== "JSXText" || !isStringEmptyOrWhiteSpace(c.value));
    const useMultilines = getUseMultilines();
    const startInfo = createInfo("startInfo");
    const endInfo = createInfo("endInfo");

    yield startInfo;
    yield* parseNode(openingElement, context);
    yield* parseJsxChildren({
        node,
        children,
        context,
        parentStartInfo: startInfo,
        parentEndInfo: endInfo,
        useMultilines
    });
    yield* parseNode(closingElement, context);
    yield endInfo;

    function getUseMultilines() {
        const firstChild = allChildren[0];
        if (firstChild != null && firstChild.type === "JSXText" && firstChild.value.indexOf("\n") >= 0)
            return true;

        return nodeHelpers.getUseNewlinesForNodes([
            openingElement,
            children[0] || closingElement
        ]);
    }
}

export interface ParseJsxChildrenOptions {
    node: babel.Node;
    children: babel.Node[];
    context: Context;
    parentStartInfo: Info;
    parentEndInfo: Info;
    useMultilines: boolean;
}

function* parseJsxChildren(options: ParseJsxChildrenOptions): PrintItemIterable {
    const { node, children, context, parentStartInfo, parentEndInfo, useMultilines } = options;
    // Need to parse the children here so they only get parsed once.
    // Nodes need to be only parsed once so that their comments don't end up in
    // the handled comments collection and the second time they're p
    // won't be parsed out.
    const parsedChildren = children.map(c => [c, makeIterableRepeatable(parseNode(c, context))] as const);

    if (useMultilines)
        yield* parseForNewLines();
    else {
        // decide whether newlines should be used or not
        yield {
            kind: PrintItemKind.Condition,
            name: "JsxChildrenNewLinesOrNot",
            condition: conditionContext => {
                // use newlines if the header is multiple lines
                if (conditionResolvers.isMultipleLines(conditionContext, parentStartInfo, conditionContext.writerInfo))
                    return true;

                // use newlines if the entire jsx element is on multiple lines
                return conditionResolvers.isMultipleLines(conditionContext, parentStartInfo, parentEndInfo);
            },
            true: parseForNewLines(),
            false: parseForSingleLine()
        };
    }

    function* parseForNewLines(): PrintItemIterable {
        yield Signal.NewLine;
        yield* withIndent(parseStatementOrMembers({
            context,
            innerComments: node.innerComments,
            items: parsedChildren,
            lastNode: undefined,
            shouldUseSpace,
            shouldUseNewLine: (previousElement, nextElement) => {
                if (nextElement.type === "JSXText")
                    return !hasNoNewlinesInLeadingWhitespace(nodeHelpers.getJsxText(nextElement));
                if (previousElement.type === "JSXText")
                    return !hasNoNewlinesInTrailingWhitespace(nodeHelpers.getJsxText(previousElement));
                return true;
            },
            shouldUseBlankLine: (previousElement, nextElement) => {
                if (previousElement.type === "JSXText")
                    return hasNewLineOccurrencesInTrailingWhitespace(nodeHelpers.getJsxText(previousElement), 2);
                if (nextElement.type === "JSXText")
                    return hasNewlineOccurrencesInLeadingWhitespace(nodeHelpers.getJsxText(nextElement), 2);
                return nodeHelpers.hasSeparatingBlankLine(previousElement, nextElement);
            }
        }));

        if (children.length > 0)
            yield Signal.NewLine;
    }

    function* parseForSingleLine(): PrintItemIterable {
        if (children.length === 0)
            yield Signal.PossibleNewLine;
        else {
            for (let i = 0; i < children.length; i++) {
                if (i > 0 && shouldUseSpace(children[i - 1], children[i]))
                    yield Signal.SpaceOrNewLine;

                yield* parsedChildren[i][1];
                yield Signal.PossibleNewLine;
            }
        }
    }

    function shouldUseSpace(previousElement: babel.Node, nextElement: babel.Node) {
        if (previousElement.type === "JSXText")
            return nodeHelpers.getJsxText(previousElement).endsWith(" ");
        if (nextElement.type === "JSXText")
            return nodeHelpers.getJsxText(nextElement).startsWith(" ");
        return false;
    }
}

function* parseStatements(block: babel.BlockStatement | babel.Program, context: Context): PrintItemIterable {
    let lastNode: babel.Node | undefined;
    for (const directive of block.directives) {
        if (lastNode != null) {
            yield Signal.NewLine;
            if (nodeHelpers.hasSeparatingBlankLine(lastNode, directive))
                yield Signal.NewLine;
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
    items: (babel.Node[]) | (readonly [babel.Node, PrintItemIterable])[];
    innerComments: ReadonlyArray<babel.Comment> | undefined | null;
    lastNode: babel.Node | undefined;
    context: Context;
    shouldUseSpace?: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
    shouldUseNewLine?: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
    shouldUseBlankLine: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
    trailingCommas?: TypeScriptConfiguration["trailingCommas"];
}

function* parseStatementOrMembers(opts: ParseStatementOrMembersOptions): PrintItemIterable {
    const { items, innerComments, context, shouldUseSpace, shouldUseNewLine, shouldUseBlankLine, trailingCommas } = opts;
    let { lastNode } = opts;

    for (const itemOrArray of items) {
        let item: babel.Node;
        let parsedNode: PrintItemIterable | undefined;
        if (itemOrArray instanceof Array) {
            item = itemOrArray[0];
            parsedNode = itemOrArray[1];
        }
        else {
            // todo: why is this assertion necessary?
            item = itemOrArray as babel.Node;
        }

        if (lastNode != null) {
            if (shouldUseNewLine == null || shouldUseNewLine(lastNode, item)) {
                yield Signal.NewLine;

                if (shouldUseBlankLine(lastNode, item))
                    yield Signal.NewLine;
            }
            else if (shouldUseSpace != null && shouldUseSpace(lastNode, item)) {
                yield Signal.SpaceOrNewLine;
            }
        }

        const endInfo = createInfo("endStatementOrMemberInfo");
        context.endStatementOrMemberInfo.push(endInfo);
        yield* parsedNode || parseNode(item, context, {
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
    if (lastNode != null)
        yield* parseTrailingCommentsAsStatements(lastNode, context);

    if (innerComments != null) {
        const result = Array.from(parseCommentCollection(innerComments, undefined, context));
        if (result.length > 0 && lastNode != null)
            yield Signal.NewLine;
        yield* result;
    }
}

function* parseTrailingCommentsAsStatements(node: babel.Node, context: Context): PrintItemIterable {
    const unhandledComments = getTrailingCommentsAsStatements(node, context);
    yield* parseCommentCollection(unhandledComments, node, context);
}

function* getTrailingCommentsAsStatements(node: babel.Node, context: Context) {
    for (const comment of getPossibleComments()) {
        if (!context.handledComments.has(comment) && node.loc!.end.line < comment.loc!.end.line)
            yield comment;
    }

    function* getPossibleComments() {
        if (node.trailingComments)
            yield* node.trailingComments;

        // if a node has a body it might have the trailing comments on the body instead
        const nodeBody = (node as babel.ClassMethod).body;
        if (nodeBody && nodeBody.trailingComments)
            yield* nodeBody.trailingComments;
    }
}

interface ParseParametersOrArgumentsOptions {
    nodes: babel.Node[];
    context: Context;
    forceMultiLineWhenMultipleLines: boolean;
    customCloseParen?: PrintItemIterable;
}

function* parseParametersOrArguments(options: ParseParametersOrArgumentsOptions): PrintItemIterable {
    const { nodes, context, customCloseParen, forceMultiLineWhenMultipleLines = false } = options;
    const startInfo = createInfo("startParamsOrArgs");
    const endInfo = createInfo("endParamsOrArgs");
    const useNewLines = getUseNewLines();

    yield* parseItems();

    function* parseItems(): PrintItemIterable {
        yield startInfo;
        yield "(";

        const paramList = makeIterableRepeatable(parseParameterList());
        yield {
            kind: PrintItemKind.Condition,
            name: "multiLineOrHanging",
            condition: multiLineOrHangingConditionResolver,
            true: surroundWithNewLines(withIndent(paramList)),
            false: paramList
        };

        if (customCloseParen)
            yield* customCloseParen;
        else
            yield ")";

        yield endInfo;
    }

    function parseParameterList(): PrintItemIterable {
        return parseCommaSeparatedValues({
            values: nodes,
            multiLineOrHangingConditionResolver,
            context
        });
    }

    function multiLineOrHangingConditionResolver(conditionContext: ResolveConditionContext) {
        if (useNewLines)
            return true;
        if (forceMultiLineWhenMultipleLines && !isSingleFunction())
            return conditionResolvers.isMultipleLines(conditionContext, startInfo, endInfo);
        return false;

        function isSingleFunction() {
            return nodes.length === 1 && (nodes[0].type === "FunctionExpression" || nodes[0].type === "ArrowFunctionExpression");
        }
    }

    function getUseNewLines() {
        if (nodes.length === 0)
            return false;

        return nodeHelpers.getUseNewlinesForNodes([getOpenParenToken(), nodes[0]]);

        function getOpenParenToken() {
            const paramHasParen = nodeHelpers.hasParentheses(nodes[0]);
            const firstOpenParen = tokenHelpers.getFirstOpenParenTokenBefore(nodes[0], context);

            // ensure this open paren is within the parent
            if (firstOpenParen != null && firstOpenParen.start < context.parent.start!)
                return undefined;

            return paramHasParen ? tokenHelpers.getFirstOpenParenTokenBefore(firstOpenParen!, context) : firstOpenParen;
        }
    }
}

export interface ParseCommaSeparatedValuesOptions {
    values: babel.Node[];
    context: Context;
    multiLineOrHangingConditionResolver: ResolveCondition;
}

function* parseCommaSeparatedValues(options: ParseCommaSeparatedValuesOptions): PrintItemIterable {
    const { values, context, multiLineOrHangingConditionResolver } = options;

    for (let i = 0; i < values.length; i++) {
        const param = values[i];
        const hasComma = i < values.length - 1;
        const parsedParam = makeIterableRepeatable(newlineGroup(parseValue(param, hasComma)));

        if (i === 0)
            yield* parsedParam;
        else {
            yield {
                kind: PrintItemKind.Condition,
                name: "multiLineOrHangingCondition",
                condition: multiLineOrHangingConditionResolver,
                true: function*(): PrintItemIterable {
                    yield Signal.NewLine;
                    yield* parsedParam;
                }(),
                false: function*(): PrintItemIterable {
                    yield Signal.SpaceOrNewLine;
                    yield* conditions.indentIfStartOfLine(parsedParam);
                }()
            };
        }
    }

    function* parseValue(param: babel.Node, hasComma: boolean): PrintItemIterable {
        yield* newlineGroup(parseNode(param, context, {
            innerParse: function*(iterator) {
                yield* iterator;

                if (hasComma)
                    yield ",";
            }
        }));
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
    const typeNodeStartInfo = createInfo("typeNodeStart");
    const typeNodeEndInfo = createInfo("typeNodeEnd");
    // this is used in the true and false condition, so make it repeatable
    const parsedTypeNodeIterator = makeIterableRepeatable(parseTypeNode());

    yield {
        kind: PrintItemKind.Condition,
        name: "newlineIfHeaderHangingAndTypeNodeMultipleLines",
        condition: conditionContext => {
            return conditionResolvers.isHanging(conditionContext, startInfo)
                && conditionResolvers.isMultipleLines(conditionContext, typeNodeStartInfo, typeNodeEndInfo);
        },
        true: function*() {
            yield Signal.NewLine;
            yield ")";
            yield* parsedTypeNodeIterator;
        }(),
        false: function*() {
            yield ")";
            yield* parsedTypeNodeIterator;
        }()
    };

    function* parseTypeNode(): PrintItemIterable {
        if (!typeNode)
            return;

        yield typeNodeStartInfo;

        if (typeNodeSeparator)
            yield* typeNodeSeparator;
        else {
            if (context.config["typeAnnotation.spaceBeforeColon"])
                yield " ";
            yield ": ";
        }

        yield* parseNode(typeNode, context);
        yield typeNodeEndInfo;
    }
}

export interface ParseNodeInParensOptions {
    firstInnerNode: babel.Node | BabelToken;
    innerIterable: PrintItemIterable;
    context: Context;
}

function* parseNodeInParens(options: ParseNodeInParensOptions): PrintItemIterable {
    const { firstInnerNode, innerIterable, context } = options;
    const openParenToken = tokenHelpers.getFirstOpenParenTokenBefore(firstInnerNode, context)!;
    const useNewLines = nodeHelpers.getUseNewlinesForNodes([openParenToken, firstInnerNode]);

    if (useNewLines)
        putDisableIndentInBagIfNecessaryForNode(firstInnerNode, context);

    yield* parseIteratorInParens(innerIterable, useNewLines, context);
}

function* parseIteratorInParens(iterator: PrintItemIterable, useNewLines: boolean, context: Context) {
    yield* newlineGroup(function*() {
        yield "(";

        if (useNewLines) {
            yield Signal.NewLine;
            yield* withIndent(iterator);
            yield Signal.NewLine;
        }
        else {
            yield* iterator;
        }

        yield ")";
    }());
}

function* parseNamedImportsOrExports(
    parentDeclaration: babel.ImportDeclaration | babel.ExportNamedDeclaration,
    namedImportsOrExports: (babel.ImportSpecifier | babel.ExportSpecifier)[],
    context: Context
): PrintItemIterable {
    if (namedImportsOrExports.length === 0)
        return;

    const useNewLines = getUseNewLines();
    const braceSeparator = useNewLines ? Signal.NewLine : (getUseSpace() ? " " : "");

    yield "{";
    yield braceSeparator;

    if (useNewLines)
        yield* withIndent(parseSpecifiers());
    else
        yield* parseSpecifiers();

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
                yield useNewLines ? Signal.NewLine : Signal.SpaceOrNewLine;
            }

            if (useNewLines)
                yield* parseNode(namedImportsOrExports[i], context);
            else
                yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(namedImportsOrExports[i], context)));
        }
    }

    function getUseSpace() {
        switch (parentDeclaration.type) {
            case "ExportNamedDeclaration":
                return context.config["exportDeclaration.spaceSurroundingNamedExports"];
            case "ImportDeclaration":
                return context.config["importDeclaration.spaceSurroundingNamedExports"];
            default:
                return assertNever(parentDeclaration);
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
                yield Signal.NewLine;
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
        yield Signal.NewLine;
}

function* parseForMemberLikeExpression(
    parent: babel.Node,
    leftNode: babel.Node,
    rightNode: babel.Node,
    isComputed: boolean,
    context: Context
): PrintItemIterable {
    const useNewline = nodeHelpers.getUseNewlinesForNodes([leftNode, rightNode]);

    yield* parseNode(leftNode, context);

    if (useNewline)
        yield Signal.NewLine;
    else
        yield Signal.PossibleNewLine;

    yield* conditions.indentIfStartOfLine(parseRightNode());

    function* parseRightNode(): PrintItemIterable {
        if (parent.type === "OptionalMemberExpression" && parent.optional) {
            yield "?";
            if (isComputed)
                yield ".";
        }

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

    yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise({
        startInfo: startHeaderInfo
    });
    yield* conditions.indentIfStartOfLine(function*() {
        // this group here will force it to put the extends or implements on a new line
        yield* newlineGroup(function*() {
            yield `${text} `;
            for (let i = 0; i < items.length; i++) {
                if (i > 0) {
                    yield ",";
                    yield Signal.SpaceOrNewLine;
                }

                yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(items[i], context)));
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
            yield Signal.NewLine;

        for (let i = 0; i < elements.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            const element = elements[i];
            const hasComma = forceTrailingCommas || i < elements.length - 1;
            yield* conditions.indentIfStartOfLine(newlineGroup(parseElement(element, hasComma)));

            if (useNewlines)
                yield Signal.NewLine;
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
    const separator = multiLine ? Signal.NewLine : " ";

    yield startInfo;
    yield "{";
    yield separator;
    yield* getInner();
    yield separator;
    yield "}";
    yield endInfo;

    function* getInner(): PrintItemIterable {
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

                yield* conditions.indentIfStartOfLine(newlineGroup(parseNode(members[i], context, {
                    innerParse: function*(iterator) {
                        yield* iterator;

                        if (trailingCommas) {
                            const forceTrailingCommas = getForceTrailingCommas(trailingCommas, multiLine);
                            if (forceTrailingCommas || i < members.length - 1)
                                yield ",";
                        }
                    }
                })));
            }
        }
    }
}

function* getWithComments(node: babel.Node, printItemIterator: PrintItemIterable, context: Context): PrintItemIterable {
    yield* parseLeadingComments(node, context);
    yield* printItemIterator;
    yield* parseTrailingComments(node, context);
}

function parseLeadingComments(node: babel.Node, context: Context) {
    return parseCommentsAsLeading(node, node.leadingComments, context);
}

function* parseCommentsAsLeading(node: babel.Node, leadingComments: readonly babel.Comment[] | null, context: Context) {
    if (!leadingComments)
        return;
    const lastComment = leadingComments[leadingComments.length - 1];
    const hasHandled = lastComment == null || context.handledComments.has(lastComment);

    yield* parseCommentCollection(leadingComments, undefined, context);

    if (lastComment != null && !hasHandled) {
        if (node.loc!.start.line > lastComment.loc!.end.line) {
            yield Signal.NewLine;

            if (node.loc!.start.line - 1 > lastComment.loc!.end.line)
                yield Signal.NewLine;
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

    yield* parseCommentsAsTrailing(node, trailingComments, context);

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
            // todo: something faster than O(n) -- slightly harder because this includes null values
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

function* parseCommentsAsTrailing(node: babel.Node, trailingComments: readonly babel.Comment[] | null, context: Context) {
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
}

function* parseCommentCollection(comments: Iterable<babel.Comment>, lastNode: babel.Node | babel.Comment | undefined, context: Context) {
    for (const comment of comments) {
        if (context.handledComments.has(comment))
            continue;

        yield* parseCommentBasedOnLastNode(comment, lastNode, context);
        lastNode = comment;
    }
}

function* parseCommentBasedOnLastNode(comment: babel.Comment, lastNode: babel.Node | babel.Comment | undefined, context: Context) {
    if (lastNode != null) {
        if (comment.loc.start.line > lastNode.loc!.end.line) {
            yield Signal.NewLine;

            if (comment.loc.start.line > lastNode.loc!.end.line + 1)
                yield Signal.NewLine;
        }
        else if (comment.type === "CommentLine")
            yield " ";
        else if (lastNode.type === "CommentBlock")
            yield " ";
    }

    yield* parseComment(comment, context);
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
        if (context.handledComments.has(trailingComment))
            continue;

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
    bodyNode: babel.Node | BabelToken;
    startHeaderInfo: Info | undefined;
    context: Context;
}

function* parseBraceSeparator(opts: ParseBraceSeparatorOptions) {
    const { bracePosition, bodyNode, startHeaderInfo, context } = opts;

    if (bracePosition === "nextLineIfHanging") {
        if (startHeaderInfo == null)
            yield " ";
        else {
            yield conditions.newlineIfHangingSpaceOtherwise({
                startInfo: startHeaderInfo
            });
        }
    }
    else if (bracePosition === "sameLine")
        yield " ";
    else if (bracePosition === "nextLine")
        yield Signal.NewLine;
    else if (bracePosition === "maintain") {
        const isExpression = typeof bodyNode.type === "string" && bodyNode.type !== "BlockStatement";
        if (!isExpression && nodeHelpers.isFirstNodeOnLine(bodyNode, context.fileText))
            yield Signal.NewLine;
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
    tokenText: "else" | "catch" | "finally",
    context: Context
): PrintItemIterable {
    if (nextControlFlowPosition === "sameLine")
        yield " ";
    else if (nextControlFlowPosition === "nextLine")
        yield Signal.NewLine;
    else if (nextControlFlowPosition === "maintain") {
        const token = getFirstControlFlowToken();
        if (token != null && nodeHelpers.isFirstNodeOnLine(token, context.fileText))
            yield Signal.NewLine;
        else
            yield " ";
    }
    else {
        assertNever(nextControlFlowPosition);
    }

    function getFirstControlFlowToken() {
        if (tokenText === "catch")
            return context.tokenFinder.getFirstTokenWithin(nodeBlock, tokenText);
        else
            return context.tokenFinder.getFirstTokenBefore(nodeBlock, tokenText);
    }
}
function* parseTypeAnnotationWithColonIfExists(node: babel.Node | null | undefined, context: Context) {
    if (node == null)
        return;

    if (context.config["typeAnnotation.spaceBeforeColon"])
        yield " ";

    yield* parseNodeWithPreceedingColon(node, context);
}

function* parseNodeWithPreceedingColon(node: babel.Node | null | undefined, context: Context) {
    if (node == null)
        return;

    yield ":";
    yield Signal.SpaceOrNewLine;
    yield* conditions.indentIfStartOfLine(parseNode(node, context));
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

function putDisableIndentInBagIfNecessaryForNode(node: babel.Node | BabelToken, context: Context) {
    if (node.type !== "LogicalExpression" && node.type !== "BinaryExpression")
        return;

    context.bag.put(BAG_KEYS.DisableIndentBool, true);
}
