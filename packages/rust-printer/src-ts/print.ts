import { PrintItemIterable } from "@dprint/types";
import * as rustPrinter from "./pkg/dprint_rust_printer";
import { preparePrintItems } from "./preparePrintItems";
import { PrintOptions } from "./PrintOptions";
import { printWriteItems } from "./printWriteItems";

/**
 * Print out the provided print items using the rust printer.
 * @param items - Items to print.
 * @param options - Options for printing.
 */
export function print(items: PrintItemIterable, options: PrintOptions) {
    const writeItems = rustPrinter.get_write_items(preparePrintItems(items), options.maxWidth, options.indentWidth, options.isTesting);
    return printWriteItems(writeItems, options);
}
