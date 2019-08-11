import * as babel from "@babel/types";
import { TokenFinder } from "./utils";
import { BabelToken } from "./BabelToken";

export function getFirstOpenBraceTokenWithin(node: babel.Node, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenWithin(node, "{");
}

export function getFirstOpenBracketTokenWithin(node: babel.Node, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenWithin(node, "[");
}

export function getFirstAngleBracketTokenBefore(node: babel.Node, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenBefore(node, "<");
}

export function getFirstNonCommentTokenBefore(node: babel.Node, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenBefore(node, isNotComment);
}

export function getFirstOpenParenTokenBefore(node: babel.Node | BabelToken, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenBefore(node, "(");
}

export function getFirstCloseParenTokenAfter(node: babel.Node, context: { tokenFinder: TokenFinder; }) {
    return context.tokenFinder.getFirstTokenAfter(node, ")");
}

function isNotComment(token: BabelToken) {
    return token.type !== "CommentLine" && token.type !== "CommentBlock";
}
