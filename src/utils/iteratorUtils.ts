export function isIterator(obj: unknown): obj is IterableIterator<unknown> {
    return Symbol.iterator in Object(obj);
}
