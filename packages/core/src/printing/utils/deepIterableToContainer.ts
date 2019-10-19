import { PrintItemIterable, PrintItemKind } from "../../types";
import { RepeatableConditionCache, RepeatableCondition } from "./RepeatableConditionCache";
import { PrintItemContainer, ConditionContainer } from "../types";

/**
 * Takes a iterable of print items and turns it into an object that can be reasoned with more easily.
 *
 * Unfortunately the code for dealing directly with iterables in the printer was extremely complicated
 * so it was simplified significantly by using arrays instead. The iterables still exist in the IR
 * generation because that syntax is nice to use there.
 * @param iterable - Iterable to convert.
 */
export function deepIterableToContainer(iterable: PrintItemIterable) {
    const repeatableConditionCache = new RepeatableConditionCache();
    return getContainer(iterable, undefined);

    function getContainer(items: PrintItemIterable, parent: PrintItemContainer | undefined) {
        const container: PrintItemContainer = {
            items: [],
            parent
        };

        for (const item of items) {
            if (typeof item === "object" && item.kind === PrintItemKind.Condition)
                container.items.push(createConditionContainer(repeatableConditionCache.getOrCreate(item), container));
            else
                container.items.push(item);
        }

        return container;
    }

    function createConditionContainer(repeatableCondition: RepeatableCondition, parent: PrintItemContainer): ConditionContainer {
        return {
            kind: PrintItemKind.Condition,
            name: repeatableCondition.name,
            condition: repeatableCondition.condition,
            originalCondition: repeatableCondition.originalCondition,
            get true() {
                // lazy initialization
                const value = repeatableCondition.true && getContainer(repeatableCondition.true, parent);
                Object.defineProperty(this, nameof(this.true), { value });
                return value;
            },
            get false() {
                // lazy initialization
                const value = repeatableCondition.false && getContainer(repeatableCondition.false, parent);
                Object.defineProperty(this, nameof(this.false), { value });
                return value;
            }
        };
    }
}
