import { PrintItemIterator } from "../types";

export function isIterator(obj: unknown): obj is Iterator<unknown> {
    return Symbol.iterator in Object(obj);
}

// not sure why this is necessary
export function isPrintItemIterator(obj: unknown): obj is PrintItemIterator {
    return isIterator(obj);
}