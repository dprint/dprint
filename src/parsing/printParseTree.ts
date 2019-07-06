import { Group, PrintItem, Behaviour, PrintItemKind, Condition, Info, PrintItemIterator, Unknown } from "../types";
import { assertNever } from "../utils";

// this is for debugging purposes
// todo: move this out

export function printParseTree(item: PrintItem) {
    return printItem(item);
}

function printItem(item: PrintItem) {
    if (typeof item === "string")
        return printString(item);
    else if (typeof item === "number")
        return printBehaviour(item);
    else if (item.kind === PrintItemKind.Group)
        return printGroup(item);
    else if (item.kind === PrintItemKind.Condition)
        return printCondition(item);
    else if (item.kind === PrintItemKind.Info)
        return printInfo(item);
    else if (item.kind === PrintItemKind.Unknown)
        return printUnknown(item);
    return assertNever(item);
}

function printString(text: string) {
    return `"${text.replace(/"/g, `\"`).replace(/\r?\n/g, "\\n")}"`;
}

function printBehaviour(behaviour: Behaviour) {
    return "Behaviour." + Behaviour[behaviour];
}

function printGroup(group: Group) {
    return "{\n"
        + addIndentation("kind: Group,\n"
            + "items: " + printItemsWithBrackets(group.items)) + "\n"
        + "}";
}

function printCondition(condition: Condition): string {
    return "{\n"
        + addIndentation("kind: Condition,\n"
            + `name: ${printString(condition.name || "")},\n`
            + "condition: " + (condition.condition instanceof Function ? "[Function]" : printCondition(condition.condition)) + ",\n"
            + "true: " + printBranch(condition.true) + ",\n"
            + "false: " + printBranch(condition.false)) + "\n"
        + "}";

    function printBranch(items: PrintItemIterator | undefined) {
        if (items == null)
            return "undefined";
        else
            return printItemsWithBrackets(items);
    }
}

function printInfo(info: Info) {
    return `{ kind: Info, name: ${printString(info.name || "")} }`;
}

function printUnknown(unknown: Unknown) {
    return "{\n"
        + addIndentation("kind: Unknown,\n"
            + "text: " + printString(unknown.text)) + "\n"
        + "}";
}

function addIndentation(text: string) {
    return text.split("\n").map(line => "  " + line).join("\n");
}

function printItemsWithBrackets(iterator: PrintItemIterator) {
    return "[\n"
        + addIndentation(printItems(iterator))
        + "\n]";
}

function printItems(iterator: PrintItemIterator) {
    const items: string[] = [];

    for (const item of iterator)
        items.push(printItem(item));

    return items.join(",\n");
}
