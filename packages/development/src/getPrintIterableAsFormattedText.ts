import { PrintItem, Signal, PrintItemKind, Condition, Info, PrintItemIterable } from "@dprint/core";
import { assertNever } from "./utils";

/** Prints out a parsed print item iterable for debugging purposes. */
export function getPrintIterableAsFormattedText(item: PrintItemIterable) {
    return printItems(item);
}

function printItems(iterable: PrintItemIterable) {
    const items: string[] = [];

    for (const item of iterable)
        items.push(printItem(item));

    return items.join(",\n");
}

function printItem(item: PrintItem) {
    if (typeof item === "string")
        return printString(item);
    else if (typeof item === "number")
        return printSignal(item);
    else if (item.kind === PrintItemKind.Condition)
        return printCondition(item);
    else if (item.kind === PrintItemKind.Info)
        return printInfo(item);
    return assertNever(item);
}

function printString(text: string) {
    return `"${text.replace(/"/g, `\"`).replace(/\r?\n/g, "\\n")}"`;
}

function printSignal(signal: Signal) {
    return "Signal." + Signal[signal];
}

function printCondition(condition: Condition): string {
    return "{\n"
        + addIndentation("kind: Condition,\n"
            + `name: ${printString(condition.name || "")},\n`
            + "condition: " + (condition.condition instanceof Function ? "[Function]" : printCondition(condition.condition)) + ",\n"
            + "true: " + printBranch(condition.true) + ",\n"
            + "false: " + printBranch(condition.false)) + "\n"
        + "}";

    function printBranch(items: PrintItemIterable | undefined) {
        if (items == null)
            return "undefined";
        else
            return printItemsWithBrackets(items);
    }
}

function printInfo(info: Info) {
    return `{ kind: Info, name: ${printString(info.name || "")} }`;
}

function addIndentation(text: string) {
    return text.split("\n").map(line => "  " + line).join("\n");
}

function printItemsWithBrackets(iterable: PrintItemIterable) {
    return "[\n"
        + addIndentation(printItems(iterable))
        + "\n]";
}
