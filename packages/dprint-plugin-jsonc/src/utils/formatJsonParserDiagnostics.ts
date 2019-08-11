import { ParseError, printParseErrorCode } from "jsonc-parser";

export function formatJsonParserDiagnostics(diagnostics: ParseError[], text: string) {
    return diagnostics.map(e => formatJsonParserDiagnostic(e, text)).join("\n");
}

export function formatJsonParserDiagnostic(diagnostic: ParseError, text: string) {
    const errorCode = printParseErrorCode(diagnostic.error);
    return `${errorCode}: ${text.substr(diagnostic.offset, diagnostic.length)} (${diagnostic.offset})`;
}
