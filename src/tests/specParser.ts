export interface Spec {
    filePath: string;
    message: string;
    fileText: string;
    expectedText: string;
    isOnly: boolean;
}

export function parseSpecs(fileText: string) {
    const lines = fileText.replace(/\r?\n/g, "\n").split("\n");
    const specStarts = getSpecStarts();
    const specs: Spec[] = [];
    let filterOnly = false;

    for (let i = 0; i < specStarts.length; i++) {
        const startIndex = specStarts[i];
        const endIndex = specStarts[i + 1] || lines.length;
        const messageLine = lines[startIndex];
        const spec = parseSingleSpec(messageLine, lines.slice(startIndex + 1, endIndex));
        if (spec.isOnly) {
            console.log(`Running only test: ${spec.message}`);
            filterOnly = true;
        }
        specs.push(spec);
    }

    return filterOnly ? specs.filter(s => s.isOnly) : specs;

    function getSpecStarts() {
        const result: number[] = [];

        if (!lines[0].startsWith("=="))
            throw new Error("All spec files should start with a message. (ex. == Message ==)");

        for (let i = 0; i < lines.length; i++) {
            if (lines[i].startsWith("=="))
                result.push(i);
        }

        return result;
    }
}

function parseSingleSpec(messageLine: string, lines: string[]): Spec {
    // this is temp code changed during a port... this should be better
    const fileText = lines.join("\n");
    const parts = fileText.split("[expect]");
    const startText = parts[0].substring(0, parts[0].length - 1); // remove last newline
    const expectedText = parts[1].substring(1, parts[1].length); // remove first newline

    return {
        filePath: "/file.ts", // todo: make configurable
        message: parseMessage(),
        fileText: startText,
        expectedText,
        isOnly: messageLine.toLowerCase().includes("(only)")
    };

    function parseMessage() {
        // this is very crude...
        return messageLine.substring(2, messageLine.length - 2).trim();
    }
}
