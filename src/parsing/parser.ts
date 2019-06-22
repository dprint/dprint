import * as babel from "@babel/types";
import { PrintItem, PrintItemKind, Group, Behaviour, Unknown, PrintItemIterator, Condition, Info } from "../types";
import { assertNever, removeStringIndentation, isPrintItemIterator } from "../utils";

interface Context {
    file: babel.File,
    fileText: string;
    log: (message: string) => void;
    options: ParseOptions;
    handledComments: Set<babel.Comment>;
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
    };

    // todo: handle no statements and only comments
    return {
        kind: PrintItemKind.Group,
        items: parseStatements(context.file.program.body, context)
    };
}

const parseObj: { [name: string]: (node: any, context: Context) => PrintItem | PrintItemIterator; } = {
    /* common */
    "BlockStatement": parseBlockStatement,
    "Identifier": parseIdentifier,
    /* declarations */
    "ExportNamedDeclaration": parseExportNamedDeclaration,
    "FunctionDeclaration": parseFunctionDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    "ImportDeclaration": parseImportDeclaration,
    /* statements */
    "IfStatement": parseIfStatement,
    /* imports */
    "ImportDefaultSpecifier": parseImportDefaultSpecifier,
    "ImportNamespaceSpecifier": parseImportNamespaceSpecifier,
    "ImportSpecifier": parseImportSpecifier,
    /* literals */
    "StringLiteral": parseStringLiteral,
    /* keywords */
    "TSStringKeyword": () => "string",
    "TSNumberKeyword": () => "number",
    "TSBooleanKeyword": () => "boolean",
    "TSAnyKeyword": () => "any",
    "TSUnknownKeyword": () => "unknown",
    "TSObjectKeyword": () => "object",
    /* types */
    "TSTypeParameter": parseTypeParameter,
    "TSUnionType": parseUnionType,
};

function parseNode(node: babel.Node | null, context: Context): Group {
    if (node == null) {
        // todo: make this a static object?
        return {
            kind: PrintItemKind.Group,
            items: []
        };
    }

    const parseFunc = parseObj[node!.type] || parseUnknownNode;
    const printItem = parseFunc(node, context);
    return {
        kind: PrintItemKind.Group,
        items: getWithComments(node, printItem, context)
    };
}

/* common */

function* parseBlockStatement(node: babel.BlockStatement, context: Context): PrintItemIterator {
    let hadCommentLine = false;
    yield "{";
    yield* getFirstLineTrailingComments();
    if (!hadCommentLine)
        yield context.options.newLineKind;
    yield* withIndent(function*() {
        yield* parseStatements(node.body, context);
    });
    yield "}";

    function* getFirstLineTrailingComments(): PrintItemIterator {
        if (!node.trailingComments)
            return;

        for (const trailingComment of node.trailingComments) {
            if (trailingComment.loc!.start.line === node.loc!.start.line) {
                if (trailingComment.type === "CommentLine")
                    hadCommentLine = true;
                yield* parseComment(node, trailingComment, context);
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
        const info: Info = {
            kind: PrintItemKind.Info
        };
        yield info;
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

        const useNewLines = useNewLinesForParameters(node.params);
        const params = parseParameters(node.params, context);
        if (useNewLines)
            yield* params;
        else
            yield* withHangingIndent(params)

        if (node.returnType && node.returnType.type !== "Noop") {
            yield ": ";
            yield parseNode(node.returnType.typeAnnotation, context);
        }
        yield newLineIfHangingSpaceOtherwise(context, info);
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

function* parseIfStatement(node: babel.IfStatement, context: Context): PrintItemIterator {
    const info: Info = { kind: PrintItemKind.Info };
    yield info;
    yield "if (";
    yield parseNode(node.test, context);
    yield ")";
    const isHangingCondition = newLineIfHangingSpaceOtherwise(context, info);
    yield isHangingCondition;

    const requireBraces = consequentRequiresBraces(node.consequent);
    if (requireBraces)
        yield "{"
    else {
        yield {
            kind: PrintItemKind.Condition,
            condition: isHangingCondition,
            true: "{"
        };
    }

    yield context.options.newLineKind;

    yield parseNode(node.consequent, context);

    if (requireBraces) {
        yield context.options.newLineKind;
        yield "}";
    }
    else {
        yield {
            kind: PrintItemKind.Condition,
            condition: isHangingCondition,
            true: [context.options.newLineKind, "}"]
        };
    }

    function consequentRequiresBraces(statement: babel.Statement) {
        if (statement.type === "BlockStatement") {
            if (statement.body.length === 1 && !hasLeadingCommentOnDifferentLine(statement.body[0]))
                return false;
            return true;
        }

        return !hasLeadingCommentOnDifferentLine(statement)
    }
}

/* literals */

function parseStringLiteral(node: babel.StringLiteral, context: Context) {
    if (context.options.singleQuotes)
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

function* parseStatements(statements: babel.Statement[], context: Context): PrintItemIterator {
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

    if (statements.length > 0)
        yield context.options.newLineKind;
}

function* parseParameters(params: babel.Node[], context: Context): PrintItemIterator {
    const useNewLines = useNewLinesForParameters(params);
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

function newLineIfHangingSpaceOtherwise(context: Context, info: Info): Condition {
    return {
        kind: PrintItemKind.Condition,
        condition: conditionContext => {
            const isHanging = conditionContext.writerInfo.lineStartIndentLevel > conditionContext.getResolvedInfo(info).lineStartIndentLevel;
            return isHanging;
        },
        true: [context.options.newLineKind],
        false: [" "]
    }
}

/* helpers */

function* getWithComments(node: babel.Node, nodePrintItem: PrintItem | PrintItemIterator, context: Context): PrintItemIterator {
    if (node.leadingComments) {
        for (const leadingComment of node.leadingComments)
            yield* parseComment(node, leadingComment, context);
    }

    if (isPrintItemIterator(nodePrintItem))
        yield* nodePrintItem
    else
        yield nodePrintItem;

    if (node.trailingComments) {
        for (const leadingComment of node.trailingComments)
            yield* parseComment(node, leadingComment, context);
    }
}

function* parseComment(node: babel.Node, comment: babel.Comment, context: Context): PrintItemIterator {
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
        if (comment.loc!.start.line < node.loc!.start.line)
            yield context.options.newLineKind;

        yield "/*";
        yield comment.value;
        yield "*/";

        if (comment.loc!.start.line < node.loc!.start.line)
            yield context.options.newLineKind;
    }

    function* parseCommentLine(comment: babel.CommentLine): PrintItemIterator {
        if (comment.loc!.start.line < node.loc!.start.line)
            yield context.options.newLineKind;
        else
            yield " ";
        yield `// ${comment.value.trim()}`;
        yield Behaviour.ExpectNewLine;
    }
}

function useNewLinesForParameters(params: babel.Node[]) {
    return getUseNewLinesForNodes(params);
}

function getUseNewLinesForNodes(nodes: babel.Node[]) {
    if (nodes.length <= 1)
        return false;
    if (nodes[0].loc!.start.line === nodes[1].loc!.start.line)
        return false;
    return true;
}

function* surroundWithNewLines(actionOrIterator: PrintItemIterator | (() => PrintItemIterator), context: Context): PrintItemIterator {
    yield context.options.newLineKind;
    if (actionOrIterator instanceof Function)
        yield* actionOrIterator()
    else
        yield* actionOrIterator;
    yield context.options.newLineKind;
}

function* withIndent(actionOrIterator: PrintItemIterator | (() => PrintItemIterator)): PrintItemIterator {
    yield Behaviour.StartIndent;
    if (actionOrIterator instanceof Function)
        yield* actionOrIterator()
    else
        yield* actionOrIterator;
    yield Behaviour.FinishIndent;
}

function* withHangingIndent(actionOrIterator: PrintItemIterator | (() => PrintItemIterator)): PrintItemIterator {
    yield Behaviour.StartHangingIndent;
    if (actionOrIterator instanceof Function)
        yield* actionOrIterator()
    else
        yield* actionOrIterator;
    yield Behaviour.FinishHangingIndent;
}

/* checks */

function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

function hasLeadingCommentOnDifferentLine(node: babel.Node) {
    return node.leadingComments != null
        && node.leadingComments.some(c => c.type === "CommentLine" || c.loc!.start.line < node.loc!.start.line);
}
