export class Bag {
    private readonly bag = new Map<string, object>();
    put(key: string, value: any) {
        this.bag.set(key, value);
    }

    take(key: string) {
        const value = this.bag.get(key);
        this.bag.delete(key);
        return value;
    }

    peek(key: string) {
        return this.bag.get(key);
    }
}
