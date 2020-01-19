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

export { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "./Configuration";
export { TypeScriptPlugin } from "./Plugin";
