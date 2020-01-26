import { PrintItem, Condition, Info, PrintItemIterable, PrintItemKind, ResolveConditionContext } from "@dprint/types";

/**
 * Prepares the print item iterable so it can be consumed for the rust code.
 */
export function preparePrintItems(items: PrintItemIterable) {
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
                    let pastIterator: PrintItem[] | undefined;
                    return () => {
                        if (pathIterator == null)
                            return;
                        if (pastIterator != null)
                            return pastIterator;
                        pastIterator = innerGetItemsAsArray(pathIterator);
                        return pastIterator;
                    };
                })(),
                enumerable: true,
                configurable: true
            });
        }

        function isInfoOrCondition(item: PrintItem): item is Condition | Info {
            return (item as any).kind != null;
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
