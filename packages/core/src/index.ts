export const version = "PACKAGE_VERSION"; // value is replaced at build time

// attempt node < 11 support
if (typeof TextEncoder === "undefined") {
    try {
        const util = require("util") as any;
        (global as any).TextEncoder = util.TextEncoder;
        (global as any).TextDecoder = util.TextDecoder;
    } catch {
        // do nothing
    }
}

export * from "./configuration";
export * from "./parsing";
export * from "./environment";
export { makeIterableRepeatable, getFileExtension, resolveNewLineKindFromText } from "./utils";
export * from "./formatFileText";
export { print, PrintOptions } from "./printer";
