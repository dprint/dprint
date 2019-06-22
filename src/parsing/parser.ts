import * as babel from "@babel/types";
import { PrintItem, PrintItemKind, Group, Separator, Unknown, GroupSeparatorKind, PrintItemIterator,
    Condition, ConditionKind } from "../types";
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
            items: [].values()
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
    yield {
        kind: PrintItemKind.Group,
        indent: true,
        items: parseStatements(node.body, context)
    };
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

        const separatorKind = getSeparatorKind();
        const braceSeparator = separatorKind === GroupSeparatorKind.NewLines ? context.options.newLineKind : " ";

        yield "{";
        yield braceSeparator;

        yield {
            kind: PrintItemKind.Group,
            indent: separatorKind === GroupSeparatorKind.NewLines,
            hangingIndent: separatorKind !== GroupSeparatorKind.NewLines,
            items: parseSpecifiers()
        };

        yield braceSeparator;
        yield "}";

        function getSeparatorKind() {
            if (namedImports.length === 1 && namedImports[0].loc!.start.line !== node.loc!.start.line)
                return GroupSeparatorKind.NewLines;
            return getSeparatorKindForNodes(namedImports);
        }

        function* parseSpecifiers(): PrintItemIterator {
            for (let i = 0; i < namedImports.length; i++) {
                if (i > 0) {
                    yield ",";
                    yield separatorKind === GroupSeparatorKind.NewLines ? context.options.newLineKind : Separator.SpaceOrNewLine;
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

function parseImportSpecifier(specifier: babel.ImportSpecifier, context: Context): Group {
    return {
        kind: PrintItemKind.Group,
        hangingIndent: true,
        items: parseItems()
    };

    function* parseItems(): PrintItemIterator {
        if (specifier.imported.start === specifier.local.start) {
            yield parseNode(specifier.imported, context)
            return;
        }

        yield parseNode(specifier.imported, context);
        yield " as ";
        yield parseNode(specifier.local, context);
    }
}

function* parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context): PrintItemIterator {
    yield "export ";
    yield parseNode(node.declaration, context);
}

function* parseFunctionDeclaration(node: babel.FunctionDeclaration, context: Context): PrintItemIterator {
    yield* parseHeader();
    yield parseNode(node.body, context);

    function* parseHeader(): PrintItemIterator {
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
        yield parseParameters(node.params, context);
        if (node.returnType && node.returnType.type !== "Noop") {
            yield ": ";
            yield parseNode(node.returnType.typeAnnotation, context);
        }
        yield Separator.NewLineIfHangingSpaceOtherwise;
    }
}

function parseTypeParameterDeclaration(declaration: babel.TypeParameterDeclaration | babel.TSTypeParameterDeclaration, context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(declaration.params);
    return {
        kind: PrintItemKind.Group,
        items: parseItems()
    };

    function* parseItems(): PrintItemIterator {
        yield "<";
        if (separatorKind === GroupSeparatorKind.NewLines)
            yield context.options.newLineKind;
        yield {
            kind: PrintItemKind.Group,
            indent: separatorKind === GroupSeparatorKind.NewLines,
            hangingIndent: separatorKind !== GroupSeparatorKind.NewLines,
            items: parseParameterList()
        };
        if (separatorKind === GroupSeparatorKind.NewLines)
            yield context.options.newLineKind;
        yield ">";
    }

    function* parseParameterList(): PrintItemIterator {
        const params = declaration.params;
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            yield parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                yield Separator.SpaceOrNewLine;
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
    yield "if (";
    yield parseNode(node.test, context);
    yield ")";
    const isHangingCondition: Condition = {
        kind: PrintItemKind.Condition,
        condition: ConditionKind.Hanging,
        true: context.options.newLineKind,
        false: " "
    }
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
            true: [context.options.newLineKind, "}"].values()
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

function parseUnionType(node: babel.TSUnionType, context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(node.types);
    return {
        kind: PrintItemKind.Group,
        hangingIndent: true,
        items: parseTypes()
    };

    function* parseTypes(): PrintItemIterator {
        for (let i = 0; i < node.types.length; i++) {
            if (i > 0) {
                yield separatorKind === GroupSeparatorKind.NewLines ? context.options.newLineKind : Separator.SpaceOrNewLine;
                yield "| ";
            }
            yield parseNode(node.types[i], context);
        }
    }
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

function parseParameters(params: babel.Node[], context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(params);
    return {
        kind: PrintItemKind.Group,
        items: parseItems()
    };

    function* parseItems(): PrintItemIterator {
        yield "(";
        if (separatorKind === GroupSeparatorKind.NewLines)
            yield context.options.newLineKind;
        yield {
            kind: PrintItemKind.Group,
            indent: separatorKind === GroupSeparatorKind.NewLines,
            hangingIndent: separatorKind !== GroupSeparatorKind.NewLines,
            items: parseParameterList()
        };
        if (separatorKind === GroupSeparatorKind.NewLines)
            yield context.options.newLineKind;
        yield ")";
    }

    function* parseParameterList(): PrintItemIterator {
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            yield parseNode(param, context);
            if (i < params.length - 1) {
                yield ",";
                yield separatorKind === GroupSeparatorKind.NewLines ? context.options.newLineKind : Separator.SpaceOrNewLine;
            }
        }
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
        yield Separator.ExpectNewLine;
    }
}

function getSeparatorKindForNodes(nodes: babel.Node[]) {
    if (nodes.length <= 1)
        return GroupSeparatorKind.Spaces;
    if (nodes[0].loc!.start.line === nodes[1].loc!.start.line)
        return GroupSeparatorKind.Spaces;
    return GroupSeparatorKind.NewLines;
}

/* checks */

function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

function hasLeadingCommentOnDifferentLine(node: babel.Node) {
    return node.leadingComments != null
        && node.leadingComments.some(c => c.type === "CommentLine" || c.loc!.start.line < node.loc!.start.line);
}

/*
function isMultiLine(nodeToCheck: babel.Node, maxWidth: number, context: Context) {
    // clone the context because of comments being added to the set
    return isFirstLineWidthAboveLength(parseNode(nodeToCheck, context.clone()), maxWidth);
}

function isFirstLineWidthAboveLength(originalPrintItem: PrintItem, originalLength: number) {
    const finalResult = isFirstLineWidthAboveLengthInternal(originalPrintItem, originalLength, undefined);
    return typeof finalResult === "number" ? false : finalResult;

    function isFirstLineWidthAboveLengthInternal(printItem: PrintItem, length: number, groupSeparatorKind?: GroupSeparatorKind): number | boolean {
        let width = 0;
        if (typeof printItem === "number") {
            const result = getSeparatorFirstLineWidth(printItem);
            if (!result)
                return false;
            width += result;
        }
        else if (typeof printItem === "string") {
            for (let i = 0; i < printItem.length; i++) {
                if (printItem[i] === "\n")
                    return false;
                width++;

                if (width > length)
                    return true;
            }
        }
        else if (isIterator(printItem)) {
            for (const item of printItem) {
                const result = isFirstLineWidthAboveLengthInternal(item, length - width, groupSeparatorKind);
                if (typeof result === "boolean")
                    return result;
                width += result;
            }
        }
        else if (printItem.kind === PrintItemKind.Group) {
            for (const item of printItem.items) {
                const result = isFirstLineWidthAboveLengthInternal(item, length - width, printItem.separatorKind);
                if (typeof result === "boolean")
                    return result;
                width += result;
            }
        }
        else if (printItem.kind === PrintItemKind.Unknown) {
            const result = isFirstLineWidthAboveLengthInternal(printItem.text, length - width, groupSeparatorKind);
            if (typeof result === "boolean")
                return result;
            width += result;
        }
        else
            assertNever(printItem);

        if (width > length)
            return true;

        return width;

        function getSeparatorFirstLineWidth(separator: Separator) {
            if (groupSeparatorKind === GroupSeparatorKind.NewLines && (separator === Separator.NewLine || separator === Separator.SpaceOrNewLine))
                return false;

            switch (separator) {
                case Separator.ExpectNewLine:
                    return false;
                case Separator.SpaceOrNewLine:
                case Separator.NewLineIfHangingSpaceOtherwise:
                    return 1;
                case Separator.NewLine:
                    return 0;
                default:
                    return assertNever(separator);
            }
        }
    }
}
*/