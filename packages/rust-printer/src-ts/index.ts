import { PrintItemIterable, PrintItemKind, Condition, ResolveConditionContext, PrintItem, Info } from "@dprint/types";
import * as rustPrinter from "./pkg/dprint_rust_printer";

/** Options for printing. */
export interface PrintOptions {
    /** The width the printer will attempt to keep the line under. */
    maxWidth: number;
    /** The number of spaces to use when indenting (unless useTabs is true). */
    indentWidth: number;
    /** Whether to use tabs for indenting. */
    useTabs: boolean;
    /** The newline character to use when doing a new line. */
    newLineKind: "\r\n" | "\n";
    /**
     * Set to true when testing in order to run additional validation on the inputted strings, which
     * ensures the printer is being used correctly.
     */
    isTesting: boolean;
}

/**
 * Print out the provided print items using the rust printer.
 * @param items - Items to print.
 * @param options - Options for printing.
 */
export function print(items: PrintItemIterable, options: PrintOptions) {
    const writeItems = rustPrinter.get_write_items(printItemsToArray(items), options.maxWidth, options.indentWidth, options.isTesting);
    return printWriteItems(writeItems, options);
}

function printWriteItems(writeItems: any[], options: PrintOptions) {
    const finalItems: string[] = [];
    const indentationText = options.useTabs ? "\t" : " ".repeat(options.indentWidth);

    for (const item of writeItems) {
        if (typeof item === "string")
            finalItems.push(item);
        else if (item === 0)
            finalItems.push(indentationText);
        else if (item === 1)
            finalItems.push(options.newLineKind);
        else if (item === 2)
            finalItems.push("\t");
        else if (item === 3)
            finalItems.push(" ");
        else
            throw new Error(`Unhandled write item: ${item}`);
    }

    return finalItems.join("");
}

function printItemsToArray(items: PrintItemIterable) {
    // The rust code requires a unique id per info and condition, so ensure
    // that all infos and conditions are given an id.
    const getNextId = (id => () => id++)(0);
    const rustContext = new class {
        private innerContext: ResolveConditionContext | undefined;

        getResolvedCondition(condition: Condition): boolean | undefined {
            updateItem(condition);
            return this.innerContext!.getResolvedCondition(condition);
        }

        getResolvedInfo(info: Info) {
            updateItem(info);
            return this.innerContext!.getResolvedInfo(info);
        }

        get writerInfo() {
            return this.innerContext!.writerInfo;
        }

        setInnerContext(context: ResolveConditionContext) {
            if (context === this)
                throw new Error("Something went wrong and the inner context was set to the same object as the outer context.");

            this.innerContext = context;
        }
    }();

    return innerGetItemsAsArray(items);

    function innerGetItemsAsArray(items: PrintItemIterable) {
        items = items instanceof Array ? items : Array.from(items);
        for (const item of items)
            updateItem(item);
        return items as PrintItem[];
    }

    function updateItem(item: PrintItem) {
        if (!isInfoOrCondition(item))
            return;
        if (hasId(item))
            return; // we've already dealt with this info or condition in the past

        // Give it an id for rust to use.
        addId(item);

        if (item.kind === PrintItemKind.Condition) {
            // define the two path properties for the rust code to use
            addPath(item, "truePath", item.true);
            addPath(item, "falsePath", item.false);

            // Update the condition resolution to always be a function (since that's what rust expects)
            if (!(item.condition instanceof Function)) {
                const checkingCondition = item.condition;
                item.condition = (context: ResolveConditionContext) => context.getResolvedCondition(checkingCondition);
            }

            // Always use the local rust context to ensure all infos and conditions are tagged with a unique
            // id when they're requested for resolution (in case they already don't have one).
            const originalResolver = item.condition;
            item.condition = (context: ResolveConditionContext) => {
                rustContext.setInnerContext(context);
                return originalResolver(rustContext);
            };
        }

        function addPath(condition: Condition, pathName: string, pathIterator: PrintItemIterable | undefined) {
            Object.defineProperty(condition, pathName, {
                get: (() => {
                    let pastIterator: PrintItemIterable | undefined;
                    return () => {
                        if (pathIterator == null)
                            return;
                        pastIterator = pastIterator || innerGetItemsAsArray(pathIterator);
                        return pastIterator;
                    };
                })(),
                enumerable: true,
                configurable: true
            });
        }

        function isInfoOrCondition(item: PrintItem): item is Condition | Info {
            return (item as any).kind != null
        }

        function hasId(item: Condition | Info) {
            return (item as any).id != null;
        }

        function addId(item: Condition | Info) {
            const id = getNextId();
            Object.defineProperty(item, "id", {
                get: () => {
                    return id;
                },
                enumerable: true,
                configurable: true
            });

            return true;
        }
    }
}