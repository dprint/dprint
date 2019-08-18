/**
 * Binary search.
 * @param items - Items to check.
 * @param compare - Comparison function. Return -1 if the value being compared to preceeds, 0 if equal, and 1 if follows.
 */
export function binarySearch<T>(items: ReadonlyArray<T>, compare: (value: T) => -1 | 0 | 1) {
    let top = items.length - 1;
    let bottom = 0;

    while (bottom <= top) {
        const mid = Math.floor((top + bottom) / 2);
        const comparisonResult = compare(items[mid]);
        if (comparisonResult === 0)
            return mid;
        else if (comparisonResult < 0)
            top = mid - 1;
        else
            bottom = mid + 1;
    }

    return -1;
}
