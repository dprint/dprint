import { throwError } from "./assertions";

export class Stack<T> {
    private readonly items: T[] = [];

    peek(): T | undefined {
        return this.items[this.items.length - 1];
    }

    push(item: T) {
        this.items.push(item);
    }

    popOrThrow() {
        const result = this.items.pop();

        if (result == null)
            return throwError("Tried to pop, but stack was empty. Maybe a pop was accidentally done elsewhere?");

        return result;
    }
}
