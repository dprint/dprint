const path = require("path");
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");

module.exports = function override(config, env) {
    // Support web assembly.
    config.plugins.push(new WasmPackPlugin({
        crateDirectory: path.resolve(__dirname, "./wasm"),
        outDir: path.resolve(__dirname, "./src/wasm")
    }));

    // Make file-loader ignore WASM files (https://stackoverflow.com/a/59720645/188246)
    const wasmExtensionRegExp = /\.wasm$/;
    config.module.rules.forEach(rule => {
        (rule.oneOf || []).forEach(oneOf => {
            if (oneOf.loader && oneOf.loader.indexOf("file-loader") >= 0)
                oneOf.exclude.push(wasmExtensionRegExp);
        });
    });

    // Disable eslint for the generated code.
    config.module.rules.forEach(rule => {
        if (rule.use && rule.use.some(u => u.options && u.options.useEslintrc != null))
            rule.exclude = path.join(path.resolve("."), "src/wasm");
    });

    return config;
};
