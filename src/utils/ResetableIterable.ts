//todo: tests
export class ResetableIterable<T> implements Iterable<T> {
    private readonly items: T[];
    private index = 0;

    constructor(internalIterator: Iterable<T>) {
        this.items = Array.from(internalIterator);
    }

    [Symbol.iterator]() {
        return {
            next: () => {
                if (this.index >= this.items.length)
                    return { value: undefined as any as T, done: true }; // typing seems to be an issue with TypeScript

                const result = { value: this.items[this.index], done: false };
                this.index++;
                return result;
            }
        };
    }

    reset() {
        this.index = 0;
    }
}
