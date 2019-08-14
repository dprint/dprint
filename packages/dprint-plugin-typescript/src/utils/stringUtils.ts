export function isStringEmptyOrWhiteSpace(text: string) {
    const hasNonWhiteSpaceChar = /\S/.test(text);
    return !hasNonWhiteSpaceChar;
}

export function hasNewlineOccurrencesInLeadingWhitespace(text: string, occurrences: number) {
    for (let i = 0; i < text.length; i++) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return false;
        if (text[i] === "\n" && --occurrences === 0)
            return true;
    }

    return false;
}

export function hasNewLineOccurrencesInTrailingWhiteSpace(text: string, occurrences: number) {
    for (let i = text.length - 1; i >= 0; i--) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return false;
        if (text[i] === "\n" && --occurrences === 0)
            return true;
    }

    return false;
}
