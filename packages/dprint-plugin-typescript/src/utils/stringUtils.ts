export function isStringEmptyOrWhiteSpace(text: string) {
    const hasNonWhiteSpaceChar = /\S/.test(text);
    return !hasNonWhiteSpaceChar;
}

export function hasNewlineOccurrencesInLeadingWhitespace(text: string, occurrences: number) {
    if (occurrences === 0)
        hasNoNewlinesInLeadingWhitespace(text);

    for (let i = 0; i < text.length; i++) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return false;
        if (text[i] === "\n" && --occurrences === 0)
            return true;
    }

    return false;
}

export function hasNoNewlinesInLeadingWhitespace(text: string) {
    for (let i = 0; i < text.length; i++) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return true;
        if (text[i] === "\n")
            return false;
    }

    return true;
}

export function hasNewLineOccurrencesInTrailingWhitespace(text: string, occurrences: number) {
    if (occurrences === 0)
        return hasNoNewlinesInTrailingWhitespace(text);

    for (let i = text.length - 1; i >= 0; i--) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return false;
        if (text[i] === "\n" && --occurrences === 0)
            return true;
    }

    return false;
}

export function hasNoNewlinesInTrailingWhitespace(text: string) {
    for (let i = text.length - 1; i >= 0; i--) {
        if (!isStringEmptyOrWhiteSpace(text[i]))
            return true;
        if (text[i] === "\n")
            return false;
    }

    return true;
}