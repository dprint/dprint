let prefixSemiColonInsertionChars: Set<string> | undefined;

export function isPrefixSemiColonInsertionChar(char: string) {
    return getPrefixSemiColonInsertionChars().has(char);
}

function getPrefixSemiColonInsertionChars() {
    if (prefixSemiColonInsertionChars == null) {
        // from: https://standardjs.com/rules.html#semicolons
        prefixSemiColonInsertionChars = new Set(["[", "(", "`", "+", "*", "/", "-", ",", "."]);
    }
    return prefixSemiColonInsertionChars;
}
