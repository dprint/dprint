export interface UnknownConfiguration {
    [configPropertyName: string]: string | number | boolean;
}

/** A parsed spec. */
export interface Spec {
    filePath: string;
    message: string;
    fileText: string;
    expectedText: string;
    isOnly: boolean;
    showTree: boolean;
    skip: boolean;
    config: UnknownConfiguration;
}

// todo: write a better parser instead of this lazy stuff

export interface ParseSpecsOptions {
    /** The default file name for a parsed spec. */
    defaultFileName: string;
}

/** Parses all the specs in the given text file. */
export function parseSpecs(fileText: string, options: ParseSpecsOptions) {
    fileText = fileText.replace(/\r?\n/g, "\n");
    const filePath = parseFilePath();
    const configResult = parseConfig();
    const lines = configResult.fileText.split("\n");
    const specStarts = getSpecStarts();
    const specs: Spec[] = [];
    let filterOnly = false;

    for (let i = 0; i < specStarts.length; i++) {
        const startIndex = specStarts[i];
        const endIndex = specStarts[i + 1] || lines.length;
        const messageLine = lines[startIndex];
        const spec = parseSingleSpec(filePath, messageLine, lines.slice(startIndex + 1, endIndex), configResult.config);

        if (spec.skip)
            continue;

        if (spec.isOnly) {
            console.log(`NOTICE!!! Running only test: ${spec.message}`);
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

    function parseFilePath() {
        if (!fileText.startsWith("--"))
            return options.defaultFileName;
        const lastIndex = fileText.indexOf("--\n", 2);
        if (lastIndex === -1)
            throw new Error("Could not find last --\\n.");

        const result = fileText.substring(2, lastIndex).trim();

        // I was lazy while writing this and this is not ideal.
        // This is done for parseConfig()
        fileText = fileText.substring(lastIndex + 3);

        return result;
    }

    function parseConfig(): { fileText: string; config: UnknownConfiguration; } {
        if (!fileText.startsWith("~~"))
            return { fileText, config: {} };
        const lastIndex = fileText.indexOf("~~\n", 2);
        if (lastIndex === -1)
            throw new Error("Canot find last ~~\\n.");
        const configText = fileText.substring(2, lastIndex).replace(/\n/g, "");
        const config: UnknownConfiguration = {};

        for (const item of configText.split(",")) {
            const firstColon = item.indexOf(":");
            const key = item.substring(0, firstColon).trim();
            const value = parseValue(item.substring(firstColon + 1).trim());

            config[key] = value;
        }

        return {
            fileText: fileText.substring(lastIndex + 3),
            config,
        };

        function parseValue(value: string) {
            const parsedInt = parseInt(value, 10);
            if (!isNaN(parsedInt))
                return parsedInt;
            else if (value.startsWith(`"`))
                return value.slice(1, -1); // strip quotes
            else if (value === "true")
                return true;
            else if (value === "false")
                return false;
            return value;
        }
    }
}

function parseSingleSpec(filePath: string, messageLine: string, lines: string[], config: UnknownConfiguration): Spec {
    // this is temp code changed during a port... this should be better
    const fileText = lines.join("\n");
    const parts = fileText.split("[expect]");
    const startText = parts[0].substring(0, parts[0].length - 1); // remove last newline
    const expectedText = parts[1].substring(1, parts[1].length); // remove first newline
    const lowerCaseMessageLine = messageLine.toLowerCase();

    return {
        filePath,
        message: parseMessage(),
        fileText: startText,
        expectedText,
        isOnly: lowerCaseMessageLine.includes("(only)"),
        skip: lowerCaseMessageLine.includes("(skip)"),
        showTree: lowerCaseMessageLine.includes("(tree)"),
        config,
    };

    function parseMessage() {
        // this is very crude...
        return messageLine.substring(2, messageLine.length - 2).trim();
    }
}
