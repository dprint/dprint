export function makeIterableRepeatable<T>(iterable: Iterable<T>): Iterable<T> {
    return new RepeatableIterable(iterable);
}

// todo: tests

export class RepeatableIterable<T> implements Iterable<T> {
    private readonly items: T[];

    constructor(internalIterator: Iterable<T>) {
        this.items = Array.from(internalIterator);
    }

    [Symbol.iterator]() {
        let index = 0;
        return {
            next: () => {
                if (index >= this.items.length)
                    return { value: undefined as any as T, done: true }; // typing seems to be an issue with TypeScript

                const result = { value: this.items[index], done: false };
                index++;
                return result;
            }
        };
    }
}
