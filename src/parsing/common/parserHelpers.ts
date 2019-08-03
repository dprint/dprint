import { PrintItemIterator, PrintItem, Signal } from "../../types";
import { BaseContext } from "./BaseContext";

export function* withIndent(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartIndent;
    yield* item;
    yield Signal.FinishIndent;
}

export function* newlineGroup(item: PrintItemIterator): PrintItemIterator {
    yield Signal.StartNewlineGroup;
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

export function* toPrintItemIterator(printItem: PrintItem): PrintItemIterator {
    yield printItem;
}

export function* surroundWithNewLines(item: PrintItemIterator, context: BaseContext): PrintItemIterator {
    yield context.newlineKind;
    yield* item;
    yield context.newlineKind;
}
