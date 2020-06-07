const MonacoWebpackPlugin = require("monaco-editor-webpack-plugin");

module.exports = function override(config, env) {
    config.plugins = [
        ...config.plugins,
        new MonacoWebpackPlugin({
            languages: ["json", "typescript"],
        }),
    ];

    return config;
};
