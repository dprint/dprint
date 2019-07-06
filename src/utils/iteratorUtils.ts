import { PrintItemIterator } from "../types";

export function isIterator(obj: unknown): obj is Iterator<unknown> {
    if (typeof obj === "string")
        return false;
    return Symbol.iterator in Object(obj);
}

// not sure why this is necessary
export function isPrintItemIterator(obj: unknown): obj is PrintItemIterator {
    return isIterator(obj);
}
