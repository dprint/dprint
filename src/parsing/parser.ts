import { PrintItem, PrintItemKind, Group, PrintItemArray, Separator, Unknown, CommentBlock, GroupSeparatorKind } from "../types";
import * as babel from "@babel/types";
import { assertNever, removeStringIndentation } from "../utils";

interface Context {
    file: babel.File,
    fileText: string;
    log: (message: string) => void;
    options: ParseOptions;
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
        options
    };

    // todo: handle no statements and only comments
    return {
        kind: PrintItemKind.Group,
        items: parseStatements(context.file.program.body, context)
    };
}

const parseObj: { [name: string]: (node: any, context: Context) => PrintItem | IterableIterator<PrintItem>; } = {
    /* common */
    "Identifier": parseIdentifier,
    /* declarations */
    "ExportNamedDeclaration": parseExportNamedDeclaration,
    "FunctionDeclaration": parseFunctionDeclaration,
    "TSTypeAliasDeclaration": parseTypeAlias,
    "ImportDeclaration": parseImportDeclaration,
    /* imports */
    "ImportDefaultSpecifier": parseImportDefaultSpecifier,
    "ImportNamespaceSpecifier": parseImportNamespaceSpecifier,
    "ImportSpecifier": parseImportSpecifier,
    /* literals */
    "StringLiteral": parseStringLiteral,
    /* keywords */
    "TSStringKeyword": parseStringKeyword,
    "TSNumberKeyword": parseNumberKeyword,
    /* types */
    "TSTypeParameter": parseTypeParameter,
    "TSUnionType": parseUnionType,
};

function parseNode(node: babel.Node | null, context: Context) {
    if (node == null)
        return [] as PrintItemArray;

    const func = parseObj[node.type];
    if (func) {
        const result = func(node, context);
        return Symbol.iterator in Object(result) ? Array.from(result as IterableIterator<PrintItem>) : result as PrintItem;
    }
    else
        return parseUnknownNode(node, context);
}

/* nodes */

function* parseImportDeclaration(node: babel.ImportDeclaration, context: Context): IterableIterator<PrintItem> {
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

    function* parseNamedImports(): IterableIterator<PrintItem> {
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
            separatorKind,
            items: Array.from(parseSpecifiers())
        };

        yield braceSeparator;
        yield "}";

        function getSeparatorKind() {
            if (namedImports.length === 1 && namedImports[0].loc!.start.line !== node.loc!.start.line)
                return GroupSeparatorKind.NewLines;
            return getSeparatorKindForNodes(namedImports);
        }

        function* parseSpecifiers(): IterableIterator<PrintItem> {
            for (let i = 0; i < namedImports.length; i++) {
                if (i > 0) {
                    yield ",";
                    yield Separator.SpaceOrNewLine;
                }
                yield parseNode(namedImports[i], context);
            }
        }
    }
}

function parseImportDefaultSpecifier(specifier: babel.ImportDefaultSpecifier, context: Context) {
    return parseNode(specifier.local, context);
}

function parseImportNamespaceSpecifier(specifier: babel.ImportNamespaceSpecifier, context: Context) {
    return [
        "* as ",
        parseNode(specifier.local, context)
    ];
}

function parseImportSpecifier(specifier: babel.ImportSpecifier, context: Context): Group {
    return {
        kind: PrintItemKind.Group,
        hangingIndent: true,
        items: Array.from(parseItems())
    };

    function* parseItems(): IterableIterator<PrintItem> {
        if (specifier.imported.start === specifier.local.start) {
            yield parseNode(specifier.imported, context)
            return;
        }

        yield parseNode(specifier.imported, context);
        yield " as ";
        yield parseNode(specifier.local, context);
    }
}

function parseExportNamedDeclaration(node: babel.ExportNamedDeclaration, context: Context) {
    return getWithComments(node, [
        "export ",
        parseNode(node.declaration, context)
    ], context);
}

function parseFunctionDeclaration(node: babel.FunctionDeclaration, context: Context) {
    return getWithComments(node, {
        kind: PrintItemKind.Group,
        items: [
            parseHeader(),
            {
                kind: PrintItemKind.Group,
                indent: true,
                items: parseStatements(node.body.body, context)
            },
            "}"
        ]
    }, context);

    function parseHeader() {
        const items: PrintItem[] = [];
        if (node.async) {
            items.push("async ");
        }
        items.push("function");
        if (node.generator)
            items.push("*");
        if (node.id) {
            items.push(" " + parseIdentifier(node.id, context))
        }
        if (node.typeParameters && node.typeParameters.type !== "Noop")
            items.push(parseTypeParameterDeclaration(node.typeParameters, context));
        items.push(parseParameters(node.params, context));
        if (node.returnType && node.returnType.type !== "Noop") {
            items.push(": ");
            items.push(parseNode(node.returnType.typeAnnotation, context));
        }
        items.push(Separator.NewLineIfHangingSpaceOtherwise);
        items.push(`{${context.options.newLineKind}`)
        return items;
    }
}

function parseTypeParameterDeclaration(declaration: babel.TypeParameterDeclaration | babel.TSTypeParameterDeclaration, context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(declaration.params);
    return {
        kind: PrintItemKind.Group,
        separatorKind,
        items: [
            "<",
            Separator.NewLine,
            {
                kind: PrintItemKind.Group,
                indent: separatorKind === GroupSeparatorKind.NewLines,
                hangingIndent: separatorKind !== GroupSeparatorKind.NewLines,
                separatorKind,
                items: parseParameterList()
            },
            Separator.NewLine,
            ">"
        ]
    };

    function parseParameterList() {
        const params = declaration.params;
        const items: PrintItem[] = [];
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            items.push(parseNode(param, context));
            if (i < params.length - 1) {
                items.push(",");
                items.push(Separator.SpaceOrNewLine)
            }
        }
        return items;
    }
}

function* parseTypeAlias(node: babel.TSTypeAliasDeclaration, context: Context): IterableIterator<PrintItem> {
    yield "type ";
    yield parseIdentifier(node.id, context);
    if (node.typeParameters)
        yield parseTypeParameterDeclaration(node.typeParameters, context);
    yield " = ";
    yield parseNode(node.typeAnnotation, context);

    if (context.options.semiColons)
        yield ";";
}

function parseIdentifier(node: babel.Identifier, context: Context) {
    return getWithComments(node, node.name, context);
}

/* literals */

function parseStringLiteral(node: babel.StringLiteral, context: Context) {
    if (context.options.singleQuotes)
        return `'${node.value.replace(/'/g, `\\'`)}'`;
    return `"${node.value.replace(/"/g, `\\"`)}"`;
}

/* keywords */

function parseStringKeyword(node: babel.TSStringKeyword, context: Context) {
    return getWithComments(node, "string", context);
}

function parseNumberKeyword(node: babel.TSNumberKeyword, context: Context) {
    return getWithComments(node, "number", context);
}

function parseComment(comment: babel.CommentBlock | babel.CommentLine, context: Context) {
    switch (comment.type) {
        case "CommentBlock":
            return parseCommentBlock(comment);
        case "CommentLine":
            return parseCommentLine(comment);
        default:
            return assertNever(comment);
    }

    function parseCommentBlock(comment: babel.CommentBlock): CommentBlock {
        const { value } = comment;
        return {
            kind: PrintItemKind.CommentBlock,
            inline: false, // todo
            isJsDoc: value.startsWith("*"),
            value // todo: make this better
        };
    }

    function parseCommentLine(comment: babel.CommentLine) {
        // todo: properly handle if this should be on its own line
        return `// ${comment.value.trim()}${context.options.newLineKind}`;
    }
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

function* parseTypeParameter(node: babel.TSTypeParameter, context: Context): IterableIterator<PrintItem> {
    yield getWithComments(node, node.name!, context);

    if (node.constraint) {
        yield {
            kind: PrintItemKind.Group,
            items: [
                " extends",
                Separator.SpaceOrNewLine,
                getWithComments(node.constraint, parseNode(node.constraint, context), context)
            ],
        }
    }

    if (node.default) {
        yield {
            kind: PrintItemKind.Group,
            items: [
                " =",
                Separator.SpaceOrNewLine,
                getWithComments(node.default, parseNode(node.default, context), context)
            ],
        }
    }
}

function parseUnionType(node: babel.TSUnionType, context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(node.types);
    return {
        kind: PrintItemKind.Group,
        hangingIndent: true,
        separatorKind,
        items: parseTypes()
    };

    function parseTypes() {
        const items: PrintItem[] = [];
        for (let i = 0; i < node.types.length; i++) {
            if (i > 0) {
                items.push(Separator.SpaceOrNewLine);
                items.push("| ");
            }
            items.push(parseNode(node.types[i], context));
        }
        return items;
    }
}

/* general */

function parseStatements(statements: babel.Statement[], context: Context) {
    const printItems: PrintItem[] = [];
    for (let i = 0; i < statements.length; i++) {
        if (i > 0) {
            if (hasBody(statements[i - 1]) || hasBody(statements[i]))
                printItems.push(context.options.newLineKind.repeat(2));
            else {
                // todo: check if there is a blank line between statements and if so, respect that
                printItems.push(context.options.newLineKind);
            }
        }

        printItems.push(parseNode(statements[i], context));
    }

    if (statements.length > 0)
        printItems.push(context.options.newLineKind);

    return printItems;
}

function parseParameters(params: babel.Node[], context: Context): Group {
    const separatorKind = getSeparatorKindForNodes(params);
    return {
        kind: PrintItemKind.Group,
        separatorKind,
        items: [
            "(",
            Separator.NewLine,
            {
                kind: PrintItemKind.Group,
                indent: separatorKind === GroupSeparatorKind.NewLines,
                hangingIndent: separatorKind !== GroupSeparatorKind.NewLines,
                separatorKind,
                items: parseParameterList()
            },
            Separator.NewLine,
            ")"
        ]
    };

    function parseParameterList() {
        const items: PrintItem[] = [];
        for (let i = 0; i < params.length; i++) {
            const param = params[i];
            items.push(parseNode(param, context));
            if (i < params.length - 1) {
                items.push(",");
                items.push(Separator.SpaceOrNewLine)
            }
        }
        return items;
    }
}

/* helpers */

function getWithComments(node: babel.Node, nodePrintItem: PrintItem, context: Context) {
    // todo: store used comments in the context
    const items: PrintItem[] = [];
    if (node.leadingComments)
        items.push(...node.leadingComments.map(c => parseComment(c as babel.CommentBlock | babel.CommentLine, context)));
    items.push(nodePrintItem);
    if (node.trailingComments)
        items.push(...node.trailingComments.map(c => parseComment(c as babel.CommentBlock | babel.CommentLine, context)));
    return items;
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