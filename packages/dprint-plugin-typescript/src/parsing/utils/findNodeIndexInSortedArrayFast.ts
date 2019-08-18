import * as babel from "@babel/types";
import { binarySearch } from "../../utils";

export function findNodeIndexInSortedArrayFast(items: babel.Node[], node: babel.Node) {
    return binarySearch(items, (value) => {
        if (node.start! < value.start!)
            return -1;
        if (node.start! === value.start!)
            return 0;
        return 1;
    });
}