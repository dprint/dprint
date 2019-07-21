import * as babel from "@babel/types";
import { ResolvedConfiguration, resolveNewLineKindFromText, Configuration } from "../configuration";
import { PrintItem, PrintItemKind, Signal, Unknown, PrintItemIterator, Condition, Info } from "../types";
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

    peek(key: string) {
        return this.bag.get(key);
    }
}

const BAG_KEYS = {
    IfStatementLastBraceCondition: "ifStatementLastBraceCondition",
    ClassDeclarationStartHeaderInfo: "classDeclarationStartHeaderInfo",
    EnumDeclarationNode: "enumDeclarationNode"
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
    newlineKind: "\r\n" | "\n";
    bag: Bag;
}

export function* parseFile(file: babel.File, fileText: string, options: ResolvedConfiguration): PrintItemIterator {
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
        bag: new Bag()
    };

    yield* parseNode(file.program, context);
    yield {
        kind: PrintItemKind.Condition,
        name: "endOfFileNewLine",
        condition: conditionContext => {
            return conditionContext.writerInfo.columnNumber > 0 || conditionContext.writerInfo.lineNumber > 0;
        },
        true: [context.newlineKind]
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
    "TSDeclareFunction": parseFunctionDeclaration,
    "ImportDeclaration": parseImportDeclaration,
    "TSEnumDeclaration": parseEnumDeclaration,
    "TSEnumMember": parseEnumMember,
    "TSImportEqualsDeclaration": parseImportEqualsDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    /* class */
    "ClassBody": parseClassBody,
    "ClassMethod": parseClassMethod,
    "TSDeclareMethod": parseClassMethod,
    "ClassProperty": parseClassProperty,
    "Decorator": parseDecorator,
    "TSParameterProperty": parseParameterProperty,
    /* statements */
    "BreakStatement": parseBreakStatement,
    "ContinueStatement": parseContinueStatement,
    "DebuggerStatement": parseDebuggerStatement,
    "Directive": parseDirective,
    "DoWhileStatement": parseDoWhileStatement,
    "EmptyStatement": parseEmptyStatement,
    "TSExportAssignment": parseExportAssignment,
    "ExpressionStatement": parseExpressionStatement,
    "IfStatement": parseIfStatement,
    "InterpreterDirective": parseInterpreterDirective,
    "LabeledStatement": parseLabeledStatement,
    "ReturnStatement": parseReturnStatement,
    "ThrowStatement": parseThrowStatement,
    "TryStatement": parseTryStatement,
    "WhileStatement": parseWhileStatement,
    "VariableDeclaration": parseVariableStatement,
    "VariableDeclarator": parseVariableDeclarator,
    /* clauses */
    "CatchClause": parseCatchClause,
    /* expressions */
    "ArrayExpression": parseArrayExpression,
    "TSAsExpression": parseAsExpression,
    "AssignmentExpression": parseAssignmentExpression,
    "AwaitExpression": parseAwaitExpression,
    "BinaryExpression": parseBinaryOrLogicalExpression,
    "CallExpression": parseCallExpression,
    "ConditionalExpression": parseConditionalExpression,
    "TSExpressionWithTypeArguments": parseExpressionWithTypeArguments,
    "LogicalExpression": parseBinaryOrLogicalExpression,
    "OptionalCallExpression": parseCallExpression,
    "TSTypeAssertion": parseTypeAssertion,
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
    /* keywords */
    "Super": () => "super",
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
    "TSVoidKeyword": () => "void",
    "VoidKeyword": () => "void",
    /* types */
    "TSArrayType": parseArrayType,
    "TSConditionalType": parseConditionalType,
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
    "DoExpression": parseUnknownNode,
    "Noop": parseUnknownNode,
    "PrivateName": parseUnknownNode,
    "PipelineBareFunction": parseUnknownNode,
    "PipelineTopicExpression": parseUnknownNode,
    "ClassPrivateMethod": parseUnknownNode,
    "ClassPrivateProperty": parseUnknownNode,
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

function* parseNode(node: babel.Node | null, context: Context): PrintItemIterator {
    if (node == null)
        return;

    // store info
    context.parentStack.push(context.currentNode);
    context.parent = context.currentNode;
    context.currentNode = node;

    // parse
    const hasParentheses = nodeHelpers.hasParentheses(node);
    const parseFunc = parseObj[node!.type] || parseUnknownNode;
    const printItem = parseFunc(node, context);

    if (hasParentheses) {
        yield Signal.StartNewlineGroup;
        yield "(";
    }

    yield* getWithComments(node!, printItem, context);

    if (hasParentheses) {
        yield ")";
        yield Signal.FinishNewLineGroup;
    }

    // replace the past info after iterating
    context.currentNode = context.parentStack.pop()!;
    context.parent = context.parentStack[context.parentStack.length - 1];
}

/* file */
function* parseProgram(node: babel.Program, context: Context): PrintItemIterator {
    if (node.interpreter) {
        yield* parseNode(node.interpreter, context);
        yield context.newlineKind;

        if (nodeHelpers.hasSeparatingBlankLine(node.interpreter, node.directives[0] || node.body[0]))
            yield context.newlineKind;
    }

    yield* parseStatements(node, context);
}

/* common */

function* parseBlockStatement(node: babel.BlockStatement, context: Context): PrintItemIterator {
    const startStatementsInfo = createInfo("startStatementsInfo");
    const endStatementsInfo = createInfo("endStatementsInfo");

    yield "{";
    yield* getFirstLineTrailingComments();
    yield context.newlineKind;
    yield startStatementsInfo;
    yield* withIndent(parseStatements(node, context));
    yield endStatementsInfo;
    yield {
        kind: PrintItemKind.Condition,
        name: "endStatementsNewLine",
        condition: conditionContext => {
            return !infoChecks.areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
        },
        true: [context.newlineKind]
    };
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
    const parent = context.parent;

    yield node.name;

    if (node.optional)
        yield "?";
    if (parent.type === "VariableDeclarator" && parent.definite)
        yield "!";

    if (node.typeAnnotation) {
        yield ": ";
        yield* parseNode(node.typeAnnotation, context);
    }

    if (parent.type === "ExportDefaultDeclaration")
        yield ";"; // todo: configuration
}

/* declarations */

function* parseClassDeclaration(node: babel.ClassDeclaration, context: Context): PrintItemIterator {
    yield* parseClassDecorators();
    yield* parseHeader();
    yield* parseNode(node.body, context);

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
            yield* parseNode(node.id, context);
        }

        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        yield* withHangingIndent(parseExtendsAndImplements());

        function* parseExtendsAndImplements(): PrintItemIterator {
            if (node.superClass) {
                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo);
                yield "extends ";
                yield* withHangingIndent(function*(): PrintItemIterator {
                    yield* parseNode(node.superClass, context);
                    if (node.superTypeParameters)
                        yield* parseNode(node.superTypeParameters, context);
                }());
            }

            if (node.implements && node.implements.length > 0) {
                yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startHeaderInfo);
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
                yield Signal.SpaceOrNewLine;
            }
            yield* parseNode(node.implements[i], context);
        }
    }
}

function* parseEnumDeclaration(node: babel.TSEnumDeclaration, context: Context): PrintItemIterator {
    const startHeaderInfo = createInfo("startHeader");
    context.bag.put(BAG_KEYS.EnumDeclarationNode, node); // for when parsing a member

    yield* parseHeader();
    yield* parseBody();

    function* parseHeader(): PrintItemIterator {
        yield startHeaderInfo;

        if (node.declare)
            yield "declare ";
        if (node.const)
            yield "const ";
        yield "enum";

        yield " ";
        yield* parseNode(node.id, context);
    }

    function parseBody(): PrintItemIterator {
        return parseMemberedBody({
            bracePosition: context.config["enumDeclaration.bracePosition"],
            context,
            node,
            members: node.members,
            startHeaderInfo,
            shouldUseBlankLine
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

function* parseEnumMember(node: babel.TSEnumMember, context: Context): PrintItemIterator {
    const parentDeclaration = context.bag.peek(BAG_KEYS.EnumDeclarationNode) as babel.TSEnumDeclaration;
    yield* parseNode(node.id, context);

    if (node.initializer)
        yield* withHangingIndent(parseInitializer(node.initializer));

    const forceTrailingCommas = getForceTrailingCommas(context.config["enumDeclaration.trailingCommas"], true);
    if (forceTrailingCommas || parentDeclaration.members[parentDeclaration.members.length - 1] !== node)
        yield ",";

    function* parseInitializer(initializer: NonNullable<babel.TSEnumMember["initializer"]>): PrintItemIterator {
        if (initializer.type === "NumericLiteral" || initializer.type === "StringLiteral")
            yield Signal.SpaceOrNewLine;
        else
            yield " ";

        yield "= ";
        yield* withHangingIndent(parseNode(initializer, context));
    }
}

function* parseExportAllDeclaration(node: babel.ExportAllDeclaration, context: Context): PrintItemIterator {
    yield "export * from ";
    yield* parseNode(node.source, context);
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

function* parseExportDefaultDeclaration(node: babel.ExportDefaultDeclaration, context: Context): PrintItemIterator {
    yield* parseDecoratorsIfClass(node.declaration, context);
    yield "export default ";
    yield* parseNode(node.declaration, context);
}

function* parseFunctionDeclaration(node: babel.FunctionDeclaration | babel.TSDeclareFunction, context: Context): PrintItemIterator {
    yield* parseHeader();
    if (node.type === "FunctionDeclaration")
        yield* parseNode(node.body, context);
    else if (context.config["functionDeclaration.semiColon"])
        yield ";";

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
            yield* parseNode(node.id, context);
        }
        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context);

        if (node.returnType) {
            yield ": ";
            yield* parseNode(node.returnType, context);
        }

        if (node.type === "FunctionDeclaration") {
            yield* parseBraceSeparator({
                bracePosition: context.config["functionDeclaration.bracePosition"],
                bodyNode: node.body,
                startHeaderInfo: functionHeaderStartInfo,
                context
            });
        }
    }
}

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): PrintItemIterator {
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

function* parseTypeParameterDeclaration(
    declaration: babel.TSTypeParameterDeclaration | babel.TSTypeParameterInstantiation | babel.TypeParameterInstantiation,
    context: Context
): PrintItemIterator {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes(declaration.params);
    yield* newlineGroup(parseItems());

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
            yield* parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                if (useNewLines)
                    yield context.newlineKind;
                else
                    yield Signal.SpaceOrNewLine;
            }
        }
    }
}

function* parseImportEqualsDeclaration(node: babel.TSImportEqualsDeclaration, context: Context): PrintItemIterator {
    if (node.isExport)
        yield "export ";

    yield "import ";
    yield* parseNode(node.id, context);
    yield " = ";
    yield* parseNode(node.moduleReference, context);

    if (context.config["importEqualsDeclaration.semiColon"])
        yield ";"
}

function* parseTypeAlias(node: babel.TSTypeAliasDeclaration, context: Context): PrintItemIterator {
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

function* parseVariableStatement(node: babel.VariableDeclaration, context: Context): PrintItemIterator {
    // note: babel calls this a declaration, but this is better named a statement (as is done in the ts compiler)
    if (node.declare)
        yield "declare ";
    yield node.kind + " ";

    yield* withHangingIndent(parseDeclarators());

    if (context.config["variableStatement.semiColon"])
        yield ";";

    function* parseDeclarators() {
        for (let i = 0; i < node.declarations.length; i++) {
            if (i > 0) {
                yield ",";
                yield Signal.SpaceOrNewLine;
            }

            yield* parseNode(node.declarations[i], context);
        }
    }
}

function* parseVariableDeclarator(node: babel.VariableDeclarator, context: Context): PrintItemIterator {
    yield* parseNode(node.id, context);

    if (node.init) {
        yield " = ";
        yield* parseNode(node.init, context);
    }
}

/* class */

function parseClassBody(node: babel.ClassBody, context: Context): PrintItemIterator {
    const startHeaderInfo = context.bag.take(BAG_KEYS.ClassDeclarationStartHeaderInfo) as Info | undefined;

    return parseMemberedBody({
        bracePosition: context.config["classDeclaration.bracePosition"],
        context,
        members: node.body,
        node,
        startHeaderInfo,
        shouldUseBlankLine: (previousMember, nextMember) => {
            return nodeHelpers.hasSeparatingBlankLine(previousMember, nextMember);
        }
    });
}

function* parseClassMethod(node: babel.ClassMethod | babel.TSDeclareMethod, context: Context): PrintItemIterator {
    yield* parseHeader();

    if (node.type === "ClassMethod")
        yield* parseNode(node.body, context);
    else if (context.config["classMethod.semiColon"])
        yield ";";

    function* parseHeader(): PrintItemIterator {
        if (node.decorators)
            yield* parseDecorators(node.decorators, context);

        const startHeaderInfo = createInfo("methodStartHeaderInfo");
        yield startHeaderInfo;

        if (node.accessibility)
            yield node.accessibility + " ";
        if (node.static)
            yield "static ";
        if (node.async)
            yield "async ";
        if (node.abstract)
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

        if (node.optional)
            yield "?";

        if (node.typeParameters)
            yield* parseNode(node.typeParameters, context);

        yield* parseParametersOrArguments(node.params, context);

        if (node.returnType) {
            yield ": ";
            yield* parseNode(node.returnType, context);
        }

        if (node.type === "ClassMethod") {
            yield* parseBraceSeparator({
                bracePosition: context.config["classMethod.bracePosition"],
                bodyNode: node.body,
                startHeaderInfo: startHeaderInfo,
                context
            });
        }
    }
}

function* parseClassProperty(node: babel.ClassProperty, context: Context): PrintItemIterator {
    if (node.decorators)
        yield* parseDecorators(node.decorators, context);

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

    if (node.typeAnnotation) {
        yield ": ";
        yield* parseNode(node.typeAnnotation, context);
    }

    if (node.value) {
        yield " = ";
        yield* parseNode(node.value, context);
    }

    if (context.config["classProperty.semiColon"])
        yield ";";
}

function* parseDecorator(node: babel.Decorator, context: Context): PrintItemIterator {
    yield "@";
    yield* withHangingIndent(parseNode(node.expression, context));
}

function* parseParameterProperty(node: babel.TSParameterProperty, context: Context): PrintItemIterator {
    if (node.accessibility)
        yield node.accessibility + " ";
    if (node.readonly)
        yield "readonly ";

    yield* parseNode(node.parameter, context);
}

/* statements */

function* parseBreakStatement(node: babel.BreakStatement, context: Context): PrintItemIterator {
    yield "break";

    if (node.label != null) {
        yield " ";
        yield* parseNode(node.label, context);
    }

    if (context.config["breakStatement.semiColon"])
        yield ";";
}

function* parseContinueStatement(node: babel.ContinueStatement, context: Context): PrintItemIterator {
    yield "continue";

    if (node.label != null) {
        yield " ";
        yield* parseNode(node.label, context);
    }

    if (context.config["continueStatement.semiColon"])
        yield ";";
}

function* parseDebuggerStatement(node: babel.DebuggerStatement, context: Context): PrintItemIterator {
    yield "debugger";
    if (context.config["debuggerStatement.semiColon"])
        yield ";";
}

function* parseDirective(node: babel.Directive, context: Context): PrintItemIterator {
    yield* parseNode(node.value, context);
    if (context.config["directive.semiColon"])
        yield ";";
}

function* parseDoWhileStatement(node: babel.DoWhileStatement, context: Context): PrintItemIterator {
    // the braces are technically optional on do while statements...
    yield "do";
    yield* parseBraceSeparator({
        bracePosition: context.config["doWhileStatement.bracePosition"],
        bodyNode: node.body,
        startHeaderInfo: undefined,
        context
    });
    yield* parseNode(node.body, context);
    yield " while (";
    yield* withHangingIndent(parseNode(node.test, context));
    yield ")";

    if (context.config["doWhileStatement.semiColon"])
        yield ";";
}

function* parseEmptyStatement(node: babel.EmptyStatement, context: Context): PrintItemIterator {
    // this could possibly return nothing when semi-colons aren't supported,
    // but I'm going to keep this in and let people do this
    yield ";";
}

function* parseExportAssignment(node: babel.TSExportAssignment, context: Context): PrintItemIterator {
    yield "export = ";
    yield* parseNode(node.expression, context);

    if (context.config["exportAssignment.semiColon"])
        yield ";";
}

function* parseExpressionStatement(node: babel.ExpressionStatement, context: Context): PrintItemIterator {
    yield* parseNode(node.expression, context);

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
            yield* parseNode(node.alternate, context);
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
        yield* parseNode(ifStatement.test, context);
        yield ")";
    }
}

function* parseInterpreterDirective(node: babel.InterpreterDirective, context: Context): PrintItemIterator {
    yield "#!";
    yield node.value;
}

function* parseLabeledStatement(node: babel.LabeledStatement, context: Context): PrintItemIterator {
    yield* parseNode(node.label, context);
    yield ":";

    // not bothering to make this configurable
    if (node.body.type === "BlockStatement")
        yield " ";
    else
        yield context.newlineKind;

    yield* parseNode(node.body, context);
}

function* parseReturnStatement(node: babel.ReturnStatement, context: Context): PrintItemIterator {
    yield "return";
    if (node.argument) {
        yield " ";
        yield* parseNode(node.argument, context);
    }

    if (context.config["returnStatement.semiColon"])
        yield ";";
}

function* parseThrowStatement(node: babel.ThrowStatement, context: Context): PrintItemIterator {
    yield "throw ";
    yield* parseNode(node.argument, context);

    if (context.config["throwStatement.semiColon"])
        yield ";";
}

function* parseTryStatement(node: babel.TryStatement, context: Context): PrintItemIterator {
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
    };

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

    function* parseBody(): PrintItemIterator {
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
            yield* withIndent(parseNode(bodyNode, context));
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
                    return !infoChecks.areInfoEqual(startStatementsInfo, endStatementsInfo, conditionContext, false);
                },
                true: [context.newlineKind]
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

function* parseArrayExpression(node: babel.ArrayExpression, context: Context): PrintItemIterator {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes(node.elements);
    const forceTrailingCommas = getForceTrailingCommas(context.config["arrayExpression.trailingCommas"], useNewlines);

    yield "[";

    if (node.elements.length > 0)
        yield* withHangingIndent(parseElements());

    yield "]";

    function* parseElements(): PrintItemIterator {
        if (useNewlines)
            yield context.newlineKind;

        for (let i = 0; i < node.elements.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            const element = node.elements[i];
            if (element != null)
                yield* parseNode(element, context);

            if (forceTrailingCommas || i < node.elements.length - 1)
                yield ",";
            if (useNewlines)
                yield context.newlineKind;
        }
    }
}

function* parseAsExpression(node: babel.TSAsExpression, context: Context): PrintItemIterator {
    yield* parseNode(node.expression, context);
    yield " as ";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseAssignmentExpression(node: babel.AssignmentExpression, context: Context): PrintItemIterator {
    yield* parseNode(node.left, context);
    yield ` ${node.operator} `;
    yield* parseNode(node.right, context);
}

function* parseAwaitExpression(node: babel.AwaitExpression, context: Context): PrintItemIterator {
    yield "await ";
    yield* parseNode(node.argument, context);
}

function* parseExpressionWithTypeArguments(node: babel.TSExpressionWithTypeArguments, context: Context): PrintItemIterator {
    yield* parseNode(node.expression, context);
    yield* parseNode(node.typeParameters, context); // arguments, not parameters
}

function* parseBinaryOrLogicalExpression(node: babel.LogicalExpression | babel.BinaryExpression, context: Context): PrintItemIterator {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes([node.left, node.right]);
    const wasLastSame = context.parent.type === node.type;

    if (wasLastSame)
        yield* parseInner();
    else
        yield* newlineGroup(withHangingIndent(parseInner()));

    function* parseInner(): PrintItemIterator {
        yield* parseNode(node.left, context);

        if (useNewLines)
            yield context.newlineKind;
        else
            yield Signal.SpaceOrNewLine;

        yield node.operator;
        yield " ";
        yield* parseNode(node.right, context);
    }
}

function* parseCallExpression(node: babel.CallExpression | babel.OptionalCallExpression, context: Context): PrintItemIterator {
    yield* parseNode(node.callee, context);

    // todo: why does this have both arguments and parameters? Seems like only type parameters are filled.
    if (node.typeArguments != null)
        throwError("Unimplemented scenario where a call expression had type arguments.");

    if (node.typeParameters)
        yield* parseNode(node.typeParameters, context);

    if (node.optional)
        yield "?.";

    yield* parseParametersOrArguments(node.arguments, context);
}

function* parseConditionalExpression(node: babel.ConditionalExpression, context: Context): PrintItemIterator {
    const useNewlines = nodeHelpers.useNewlinesForParametersOrArguments([node.test, node.consequent])
        || nodeHelpers.useNewlinesForParametersOrArguments([node.consequent, node.alternate]);
    const startInfo = createInfo("startConditionalExpression");
    const endInfo = createInfo("endConditionalExpression");

    yield startInfo;
    yield* newlineGroup(parseNode(node.test, context));
    yield* withHangingIndent(parseConsequentAndAlternate());

    function* parseConsequentAndAlternate() {
        if (useNewlines)
            yield context.newlineKind;
        else
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo, endInfo);

        yield "? ";
        yield* newlineGroup(parseNode(node.consequent, context));

        if (useNewlines)
            yield context.newlineKind;
        else
            yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo, endInfo);

        yield ": ";
        yield* newlineGroup(parseNode(node.alternate, context));
        yield endInfo;
    }
}

function* parseTypeAssertion(node: babel.TSTypeAssertion, context: Context): PrintItemIterator {
    yield "<";
    yield* parseNode(node.typeAnnotation, context);
    yield "> ";
    yield* parseNode(node.expression, context);
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
    yield* parseNode(specifier.local, context);
}

function* parseImportSpecifier(specifier: babel.ImportSpecifier, context: Context): PrintItemIterator {
    if (specifier.imported.start === specifier.local.start) {
        yield* parseNode(specifier.imported, context);
        return;
    }

    yield* parseNode(specifier.imported, context);
    yield " as ";
    yield* parseNode(specifier.local, context);
}

/* exports */

function* parseExportDefaultSpecifier(node: babel.ExportDefaultSpecifier, context: Context): PrintItemIterator {
    yield "default ";
    yield* parseNode(node.exported, context);
}

function* parseExportNamespaceSpecifier(node: babel.ExportNamespaceSpecifier, context: Context): PrintItemIterator {
    yield "* as ";
    yield* parseNode(node.exported, context);
}

function* parseExportSpecifier(specifier: babel.ExportSpecifier, context: Context): PrintItemIterator {
    if (specifier.local.start === specifier.exported.start) {
        yield* parseNode(specifier.local, context);
        return;
    }

    yield* parseNode(specifier.local, context);
    yield " as ";
    yield* parseNode(specifier.exported, context);
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

function parseStringOrDirectiveLiteral(node: babel.StringLiteral | babel.DirectiveLiteral, context: Context) {
    // do not use node.value because it will not keep escaped characters as escaped characters
    const stringValue = context.fileText.substring(node.start! + 1, node.end! - 1);
    if (context.config.singleQuotes)
        return `'${stringValue.replace(/'/g, `\\'`)}'`;
    return `"${stringValue.replace(/"/g, `\\"`)}"`;
}

function parseNotSupportedFlowNode(node: babel.Node, context: Context): Unknown {
    return parseUnknownNodeWithMessage(node, context, "Flow node types are not supported");
}

function parseUnknownNode(node: babel.Node, context: Context): Unknown {
    return parseUnknownNodeWithMessage(node, context, "Not implemented node type");
}

function parseUnknownNodeWithMessage(node: babel.Node, context: Context, message: string): Unknown {
    const nodeText = context.fileText.substring(node.start!, node.end!);

    context.log(`${message}: ${node.type} (${nodeText.substring(0, 100)})`);

    return {
        kind: PrintItemKind.Unknown,
        text: nodeText
    };
}

/* types */

function* parseArrayType(node: babel.TSArrayType, context: Context): PrintItemIterator {
    yield* parseNode(node.elementType, context);
    yield "[]";
}

function* parseConditionalType(node: babel.TSConditionalType, context: Context): PrintItemIterator {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes([node.checkType, node.falseType]);
    if (context.parent.type === "TSConditionalType")
        yield* innerParse();
    else
        yield* withHangingIndent(innerParse());

    function* innerParse(): PrintItemIterator {
        yield* withHangingIndent(newlineGroup(parseMainArea()));
        yield* parseFalseType();

        function* parseMainArea(): PrintItemIterator {
            yield* newlineGroup(parseNode(node.checkType, context));
            yield Signal.SpaceOrNewLine;
            yield "extends "
            yield* newlineGroup(parseNode(node.extendsType, context));
            yield Signal.SpaceOrNewLine;
            yield "? ";
            yield* newlineGroup(parseNode(node.trueType, context));
        }

        function* parseFalseType(): PrintItemIterator {
            if (useNewlines)
                yield context.newlineKind;
            else
                yield Signal.SpaceOrNewLine;
            yield ": ";
            yield* newlineGroup(parseNode(node.falseType, context));
        }
    }
}

function* parseFunctionType(node: babel.TSFunctionType, context: Context): PrintItemIterator {
    yield* parseNode(node.typeParameters, context);
    yield* parseParametersOrArguments(node.parameters, context);
    yield " => ";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseImportType(node: babel.TSImportType, context: Context): PrintItemIterator {
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

function* parseIndexedAccessType(node: babel.TSIndexedAccessType, context: Context): PrintItemIterator {
    yield* parseNode(node.objectType, context);
    yield "[";
    yield* parseNode(node.indexType, context);
    yield "]";
}

function* parseInferType(node: babel.TSInferType, context: Context): PrintItemIterator {
    yield "infer ";
    yield* parseNode(node.typeParameter, context);
}

function* parseLiteralType(node: babel.TSLiteralType, context: Context): PrintItemIterator {
    yield* parseNode(node.literal, context);
}

function* parseMappedType(node: babel.TSMappedType, context: Context): PrintItemIterator {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes([getFirstOpenBraceToken(node, context), node.typeParameter]);
    const startInfo = createInfo("startMappedType");
    yield startInfo;
    yield "{";

    yield* withHangingIndent(parseLayout());

    yield conditions.newlineIfMultipleLinesSpaceOrNewlineOtherwise(context, startInfo)
    yield "}";

    function* parseLayout(): PrintItemIterator {
        if (useNewLines)
            yield context.newlineKind;
        else
            yield Signal.SpaceOrNewLine;

        yield* newlineGroup(parseBody());
    }

    function* parseBody(): PrintItemIterator {
        if (node.readonly)
            yield "readonly ";

        yield "[";
        yield* parseNode(node.typeParameter, context);
        yield "]";
        if (node.optional)
            yield "?";

        if (node.typeAnnotation) {
            yield ":";
            yield* newlineGroup(withHangingIndent(function*(): PrintItemIterator {
                yield Signal.SpaceOrNewLine;
                yield* parseNode(node.typeAnnotation, context);
            }()));
        }

        if (context.config["mappedType.semiColon"])
            yield ";";
    }
}

function* parseOptionalType(node: babel.TSOptionalType, context: Context): PrintItemIterator {
    yield* parseNode(node.typeAnnotation, context);
    yield "?";
}

function* parseParenthesizedType(node: babel.TSParenthesizedType, context: Context): PrintItemIterator {
    yield "(";
    yield* newlineGroup(parseNode(node.typeAnnotation, context));
    yield ")";
}

function* parseQualifiedName(node: babel.TSQualifiedName, context: Context): PrintItemIterator {
    yield* parseNode(node.left, context);
    yield ".";
    yield* parseNode(node.right, context);
}

function* parseRestType(node: babel.TSRestType, context: Context): PrintItemIterator {
    yield "...";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTupleType(node: babel.TSTupleType, context: Context): PrintItemIterator {
    const useNewlines = nodeHelpers.getUseNewlinesForNodes(node.elementTypes);
    const forceTrailingCommas = getForceTrailingCommas(context.config["tupleType.trailingCommas"], useNewlines);

    yield "[";

    if (node.elementTypes.length > 0)
        yield* withHangingIndent(parseElements());

    yield "]";

    function* parseElements(): PrintItemIterator {
        if (useNewlines)
            yield context.newlineKind;

        for (let i = 0; i < node.elementTypes.length; i++) {
            if (i > 0 && !useNewlines)
                yield Signal.SpaceOrNewLine;

            yield* parseNode(node.elementTypes[i], context);

            if (forceTrailingCommas || i < node.elementTypes.length - 1)
                yield ",";
            if (useNewlines)
                yield context.newlineKind;
        }
    }
}

function* parseTypeAnnotation(node: babel.TSTypeAnnotation, context: Context): PrintItemIterator {
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeOperator(node: babel.TSTypeOperator, context: Context): PrintItemIterator {
    if (node.operator)
        yield `${node.operator} `;

    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeParameter(node: babel.TSTypeParameter, context: Context): PrintItemIterator {
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

function* parseTypePredicate(node: babel.TSTypePredicate, context: Context): PrintItemIterator {
    yield* parseNode(node.parameterName, context);
    yield " is ";
    yield* parseNode(node.typeAnnotation, context);
}

function* parseTypeQuery(node: babel.TSTypeQuery, context: Context): PrintItemIterator {
    yield "typeof ";
    yield* parseNode(node.exprName, context);
}

function* parseTypeReference(node: babel.TSTypeReference, context: Context): PrintItemIterator {
    yield* parseNode(node.typeName, context);
    yield* parseNode(node.typeParameters, context);
}

function* parseUnionOrIntersectionType(node: babel.TSUnionType | babel.TSIntersectionType, context: Context): PrintItemIterator {
    const useNewLines = nodeHelpers.getUseNewlinesForNodes(node.types);
    yield* withHangingIndent(function*() {
        for (let i = 0; i < node.types.length; i++) {
            if (i > 0) {
                yield useNewLines ? context.newlineKind : Signal.SpaceOrNewLine;
                if (node.type === "TSUnionType")
                    yield "| ";
                else
                    yield "& ";
            }
            yield* parseNode(node.types[i], context);
        }
    }());
}

/* general */

interface ParseMemberedBodyOptions {
    node: babel.Node;
    members: babel.Node[];
    context: Context;
    startHeaderInfo: Info | undefined;
    bracePosition: NonNullable<Configuration["bracePosition"]>;
    shouldUseBlankLine: (previousMember: babel.Node, nextMember: babel.Node) => boolean;
}

function* parseMemberedBody(opts: ParseMemberedBodyOptions): PrintItemIterator {
    const { node, members, context, startHeaderInfo, bracePosition, shouldUseBlankLine } = opts;

    yield* parseBraceSeparator({
        bracePosition,
        bodyNode: getFirstOpenBraceToken(node, context) || node,
        startHeaderInfo,
        context
    });

    yield "{";
    yield* withIndent(parseBody());
    yield context.newlineKind;
    yield "}";

    function* parseBody(): PrintItemIterator {
        if (members.length > 0 || node.innerComments != null && node.innerComments.length > 0)
            yield context.newlineKind;
        yield* parseStatementOrMembers({
            items: members,
            innerComments: node.innerComments,
            lastNode: undefined,
            context,
            shouldUseBlankLine
        });
    }
}

function* parseStatements(block: babel.BlockStatement | babel.Program, context: Context): PrintItemIterator {
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
}

function* parseStatementOrMembers(opts: ParseStatementOrMembersOptions): PrintItemIterator {
    const { items, innerComments, context, shouldUseBlankLine } = opts;
    let { lastNode } = opts;

    for (const item of items) {
        if (lastNode != null) {
            yield context.newlineKind;

            if (shouldUseBlankLine(lastNode, item))
                yield context.newlineKind;
        }

        yield* parseNode(item, context);
        lastNode = item;
    }

    // get the trailing comments on separate lines of the last node
    if (lastNode != null && lastNode.trailingComments != null) {
        const unHandledComments = lastNode.trailingComments.filter(c => !context.handledComments.has(c));
        if (unHandledComments.length > 0) {
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

function* parseParametersOrArguments(params: babel.Node[], context: Context): PrintItemIterator {
    const useNewLines = nodeHelpers.useNewlinesForParametersOrArguments(params);
    yield* newlineGroup(parseItems());

    function* parseItems(): PrintItemIterator {
        yield "(";

        if (useNewLines)
            yield* surroundWithNewLines(withIndent(parseParameterList()), context);
        else
            yield* withHangingIndent(parseParameterList());

        yield ")";
    }

    function* parseParameterList(): PrintItemIterator {
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            yield* parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                yield useNewLines ? context.newlineKind : Signal.SpaceOrNewLine;
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
    const braceSeparator = useNewLines ? context.newlineKind : " ";

    yield "{";
    yield braceSeparator;

    if (useNewLines)
        yield* withIndent(parseSpecifiers());
    else
        yield* newlineGroup(withHangingIndent(parseSpecifiers()));

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
                yield useNewLines ? context.newlineKind : Signal.SpaceOrNewLine;
            }
            yield* parseNode(namedImportsOrExports[i], context);
        }
    }
}

/* helpers */

function* parseDecoratorsIfClass(declaration: babel.Node | undefined | null, context: Context): PrintItemIterator {
    if (declaration == null || declaration.type !== "ClassDeclaration")
        return;

    if (declaration.decorators != null)
        yield* parseDecorators(declaration.decorators, context);
}

function* parseDecorators(decorators: babel.Decorator[], context: Context): PrintItemIterator {
    if (decorators.length === 0)
        return;

    const useNewlines = nodeHelpers.getUseNewlinesForNodes(decorators);

    for (let i = 0; i < decorators.length; i++) {
        if (i > 0) {
            if (useNewlines)
                yield context.newlineKind;
            else
                yield Signal.SpaceOrNewLine;
        }

        yield* newlineGroup(parseNode(decorators[i], context));
    }

    yield context.newlineKind;
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

    yield* parseCommentCollection(node.leadingComments, undefined, context);

    if (lastComment != null && !hasHandled && node.loc!.start.line > lastComment.loc!.end.line) {
        yield context.newlineKind;

        if (node.loc!.start.line - 1 > lastComment.loc!.end.line)
            yield context.newlineKind;
    }
}

function* parseTrailingComments(node: babel.Node, context: Context) {
    if (!node.trailingComments)
        return;

    // use the roslyn definition of trailing comments
    const trailingCommentsOnSameLine = node.trailingComments.filter(c => c.loc!.start.line === node.loc!.end.line);
    yield* parseCommentCollection(trailingCommentsOnSameLine, node, context);
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
        yield Signal.ExpectNewLine;
    }
}

interface ParseBraceSeparatorOptions {
    bracePosition: NonNullable<Configuration["bracePosition"]>;
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
    nextControlFlowPosition: NonNullable<Configuration["nextControlFlowPosition"]>,
    nodeBlock: babel.Node,
    tokenText: string,
    context: Context
): PrintItemIterator {
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

function* surroundWithNewLines(item: PrintItemIterator | (() => PrintItemIterator), context: Context): PrintItemIterator {
    yield context.newlineKind;
    if (item instanceof Function)
        yield* item();
    else if (isPrintItemIterator(item))
        yield* item;
    else
        yield item;
    yield context.newlineKind;
}

function* withIndent(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartIndent;
    yield* item;
    yield Signal.FinishIndent;
}

function* withHangingIndent(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartHangingIndent;
    yield* item;
    yield Signal.FinishHangingIndent;
}

function* newlineGroup(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartNewlineGroup;
    yield* item;
    yield Signal.FinishNewLineGroup;
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

function getFirstOpenBraceToken(node: babel.Node, context: Context) {
    // todo: something faster than O(n)
    const tokenText = "{";
    return nodeHelpers.getFirstToken(context.file, token => {
        if (token.start < node.start!)
            return false;
        if (token.start > node.end!)
            return "stop";
        if (token.type == null)
            return false;

        return token.type.label === tokenText;
    });
}

function getForceTrailingCommas(option: NonNullable<Configuration["trailingCommas"]>, useNewlines: boolean) {
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

/* factory functions */

function createInfo(name: string): Info {
    return {
        kind: PrintItemKind.Info,
        name
    };
}
