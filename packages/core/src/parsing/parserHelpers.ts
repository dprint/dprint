import { PrintItemIterable, PrintItem, Signal, PrintItemKind, Info } from "../types";

export namespace parserHelpers {
    export function* withIndent(item: PrintItemIterable): PrintItemIterable {
        yield Signal.StartIndent;
        yield* item;
        yield Signal.FinishIndent;
    }

    export function* newlineGroup(item: PrintItemIterable): PrintItemIterable {
        yield Signal.StartNewLineGroup;
        yield* item;
        yield Signal.FinishNewLineGroup;
    }

    export function* prependToIterableIfHasItems<T>(iterable: Iterable<T>, ...items: T[]) {
        let found = false;
        for (const item of iterable) {
            if (!found) {
                yield* items;
                found = true;
            }
            yield item;
        }
    }

    export function* toPrintItemIterable(printItem: PrintItem): PrintItemIterable {
        yield printItem;
    }

    export function* surroundWithNewLines(item: PrintItemIterable): PrintItemIterable {
        yield Signal.NewLine;
        yield* item;
        yield Signal.NewLine;
    }

    /**
     * Reusable function for parsing a js-like single line comment (ex. // comment)
     * @param rawCommentValue - The comment value without the leading two slashes.
     */
    export function parseJsLikeCommentLine(rawCommentValue: string) {
        const nonSlashIndex = getFirstNonSlashIndex();
        const startTextIndex = rawCommentValue[nonSlashIndex] === " " ? nonSlashIndex + 1 : nonSlashIndex;
        const commentValue = rawCommentValue.substring(startTextIndex).trimRight();
        const prefix = "//" + rawCommentValue.substring(0, nonSlashIndex);

        return prefix + (commentValue.length > 0 ? ` ${commentValue}` : "");

        function getFirstNonSlashIndex() {
            for (let i = 0; i < rawCommentValue.length; i++) {
                if (rawCommentValue[i] !== "/")
                    return i;
            }

            return rawCommentValue.length;
        }
    }

    export function createInfo(name: string): Info {
        return {
            kind: PrintItemKind.Info,
            name
        };
    }

    /**
     * Takes a string that could contain tabs or newlines and breaks it up into
     * the correct newlines and tabs.
     * @param text - Text to break up.
     */
    export function* parseRawString(text: string): PrintItemIterable {
        let hasIgnoredIndent = false;

        const lines = text.split(/\r?\n/);
        for (let i = 0; i < lines.length; i++) {
            if (i > 0) {
                if (!hasIgnoredIndent) {
                    yield Signal.StartIgnoringIndent;
                    hasIgnoredIndent = true;
                }

                yield Signal.NewLine;
            }
            yield* parseLine(lines[i]);
        }

        if (hasIgnoredIndent)
            yield Signal.FinishIgnoringIndent;

        function* parseLine(line: string): PrintItemIterable {
            const parts = line.split("\t");
            for (let i = 0; i < parts.length; i++) {
                if (i > 0)
                    yield Signal.Tab;
                yield* parts[i];
            }
        }
    }
}
