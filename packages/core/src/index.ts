export const version = "PACKAGE_VERSION"; // value is replaced at build time

export * from "./configuration";
export * from "./types";
export * from "./parsing";
export * from "./Environment";
export * from "./plugins";
export { makeIterableRepeatable, getFileExtension, resolveNewLineKindFromText } from "./utils";
export * from "./formatFileText";
