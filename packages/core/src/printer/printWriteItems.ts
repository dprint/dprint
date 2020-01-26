import { PrintOptions } from "./PrintOptions";

export function printWriteItems(writeItems: any[], options: PrintOptions) {
    const finalItems: string[] = [];
    const indentationText = options.useTabs ? "\t" : " ".repeat(options.indentWidth);

    for (const item of writeItems) {
        if (typeof item === "string")
            finalItems.push(item);
        else if (item instanceof Array)
            finalItems.push(indentationText.repeat(item[0]));
        else if (item === 0)
            finalItems.push(options.newLineKind);
        else if (item === 1)
            finalItems.push("\t");
        else if (item === 2)
            finalItems.push(" ");
        else
            throw new Error(`Unhandled write item: ${item}`);
    }

    return finalItems.join("");
}
