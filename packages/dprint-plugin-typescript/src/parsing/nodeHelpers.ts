import * as babel from "@babel/types";
import { BaseContext } from "@dprint/core";

export function hasBody(node: babel.Node) {
    return (node as any as babel.ClassDeclaration).body != null;
}

export function hasSeparatingBlankLine(nodeA: babel.Node | babel.Comment, nodeB: babel.Node | babel.Comment | undefined) {
    if (nodeB == null)
        return false;

    return getNodeBStartLine() > nodeA.loc!.end.line + 1;

    function getNodeBStartLine() {
        const leadingComments = (nodeB! as babel.Node).leadingComments;

        if (leadingComments != null) {
            for (const leadingComment of leadingComments) {
                const commentStartLine = leadingComment.loc!.start.line;
                if (commentStartLine > nodeA.loc!.end.line)
                    return commentStartLine;
            }
        }

        return nodeB!.loc!.start.line;
    }
}

export function getLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    if (node.leadingComments == null)
        return undefined;

    for (const comment of node.leadingComments) {
        if (commentsToIgnore != null && commentsToIgnore.includes(comment))
            continue;

        if (comment.loc!.start.line < node.loc!.start.line)
            return comment;
    }

    return undefined;
}

export function hasLeadingCommentOnDifferentLine(node: babel.Node, commentsToIgnore?: ReadonlyArray<babel.Comment>) {
    return getLeadingCommentOnDifferentLine(node, commentsToIgnore) != null;
}

export function getUseNewlinesForNodes(nodes: ReadonlyArray<babel.Node | BabelToken | null | undefined>) {
    const nonNullNodes = getNodes();
    const firstNode = nonNullNodes.next().value;
    const secondNode = nonNullNodes.next().value;

    if (firstNode == null || secondNode == null || firstNode.loc!.end.line === secondNode.loc!.start.line)
        return false;

    return true;

    function* getNodes() {
        for (const node of nodes) {
            if (node != null)
                yield node;
        }
    }
}

export function isFirstNodeOnLine(node: babel.Node | BabelToken, context: BaseContext) {
    for (let i = node.start! - 1; i >= 0; i--) {
        const char = context.fileText[i];
        if (char === " " || char === "\t")
            continue;

        return char === "\n";
    }

    return true;
}

export interface BabelToken {
    start: number;
    end: number;
    value?: string;
    type?: {
        label: string;
    } | "CommentLine" | "CommentBlock";
    loc: babel.Node["loc"];
}

export function getFirstToken(file: babel.File, isMatch: (token: BabelToken) => boolean | "stop") {
    const tokens = file.tokens as BabelToken[];
    for (let i = 0; i < tokens.length; i++) {
        const token = tokens[i];
        const result = isMatch(token);
        if (result === true)
            return token;
        else if (result === "stop")
            return undefined;
    }

    return undefined;
}

export function getLastToken(file: babel.File, isMatch: (token: BabelToken) => boolean | "stop") {
    const tokens = file.tokens as BabelToken[];
    for (let i = tokens.length - 1; i >= 0; i--) {
        const token = tokens[i];
        const result = isMatch(token);
        if (result === true)
            return token;
        else if (result === "stop")
            return undefined;
    }

    return undefined;
}

export function hasParentheses(node: babel.Node): boolean {
    const extra = (node as any).extra;
    if (extra == null)
        return false;
    return extra.parenthesized || false;
}
