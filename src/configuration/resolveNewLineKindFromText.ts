import * as os from "os";

export function resolveNewLineKindFromText(text: string) {
    for (let i = 0; i < text.length; i++) {
        if (text[i] === "\n")
            return text[i - 1] === "\r" ? "\r\n" : "\n";
    }

    return os.EOL === "\r\n" ? "\r\n" : "\n";
}
