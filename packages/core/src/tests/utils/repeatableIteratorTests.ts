import { expect } from "chai";
import { RepeatableIterable, makeIterableRepeatable } from "../../utils";

describe(nameof(RepeatableIterable), () => {
    it("should be able to iterate multiple times", () => {
        const iterable = new RepeatableIterable(createIterable());
        expect(Array.from(iterateIterable(iterable))).to.deep.equal([1, 2]);
        expect(Array.from(iterateIterable(iterable))).to.deep.equal([1, 2]);
    });
});

describe(nameof(makeIterableRepeatable), () => {
    it("should be able to iterate multiple times", () => {
        const iterable = makeIterableRepeatable(createIterable());
        expect(Array.from(iterateIterable(iterable))).to.deep.equal([1, 2]);
        expect(Array.from(iterateIterable(iterable))).to.deep.equal([1, 2]);
    });
});


function* createIterable() {
    yield 1;
    yield 2;
}

function* iterateIterable<T>(iterable: Iterable<T>) {
    for (const item of iterable)
        yield item;
}