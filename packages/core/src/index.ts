export const version = "PACKAGE_VERSION"; // value is replaced at build time

export * from "./configuration";
export * from "./types";
export * from "./parsing";
export * from "./environment";
export * from "./Plugin";
export { makeIterableRepeatable, getFileExtension, resolveNewLineKindFromText } from "./utils";
export * from "./formatFileText";
