export const version = "PACKAGE_VERSION"; // value is replaced at build time

export * from "./configuration";
export * from "./parsing";
export * from "./environment";
export { makeIterableRepeatable, getFileExtension, resolveNewLineKindFromText } from "./utils";
export * from "./formatFileText";
export { PrintOptions } from "./printing";
