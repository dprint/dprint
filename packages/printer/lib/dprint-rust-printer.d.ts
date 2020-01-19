// dprint-ignore-file
import { PrintItemIterable } from "@dprint/types";

/**
 * Print out the provided print items using the rust printer.
 * @param items - Items to print.
 * @param options - Options for printing.
 */
export declare function print(items: PrintItemIterable, options: PrintOptions): string;

/** Options for printing. */
export interface PrintOptions {
    /** The width the printer will attempt to keep the line under. */
    maxWidth: number;
    /** The number of spaces to use when indenting (unless useTabs is true). */
    indentWidth: number;
    /** Whether to use tabs for indenting. */
    useTabs: boolean;
    /** The newline character to use when doing a new line. */
    newLineKind: "\r\n" | "\n";
    /**
     * Set to true when testing in order to run additional validation on the inputted strings, which
     * ensures the printer is being used correctly.
     */
    isTesting: boolean;
}
