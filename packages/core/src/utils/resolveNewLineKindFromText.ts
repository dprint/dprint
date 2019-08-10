import * as os from "os";

/**
 * Gets the last newline character from the provided text or returns the
 * system's newline character if no newline exists.
 * @param text - Text to inspect.
 */
export function resolveNewLineKindFromText(text: string) {
    for (let i = text.length - 1; i >= 0; i--) {
        if (text[i] === "\n")
            return text[i - 1] === "\r" ? "\r\n" : "\n";
    }

    return os.EOL === "\r\n" ? "\r\n" : "\n";
}
