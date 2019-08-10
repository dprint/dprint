import { Plugin } from "./Plugin";
import { ResolvedGlobalConfiguration } from "../configuration";

export interface ResolvePluginsResult {
    plugins: Plugin[];
    diagnostics: ResolvePluginDiagnostic[];
}

export interface ResolvePluginDiagnostic {
    /** The name of the plugin. */
    pluginName: string;
    /** The diagnostic message. */
    message: string;
}

export async function resolvePlugins(pluginNames: string[]): Promise<ResolvePluginsResult> {
    const promises = pluginNames.map(pluginName => new Promise<unknown>((resolve, reject) => {
        // todo: use a dynamic import? (that's why this currently uses a promise)
        try {
            resolve(require(pluginName));
        } catch (err) {
            reject(err);
        }
    }));
    const result: ResolvePluginsResult = {
        plugins: [],
        diagnostics: []
    };

    for (let i = 0; i < promises.length; i++) {
        const pluginName = pluginNames[i];
        const promise = promises[i];

        try {
            const promiseResult = await promise;
            const pluginCtor = promiseResult && (promiseResult as any).default as any;
            if (pluginCtor) {
                result.diagnostics.push({
                    pluginName,
                    message: `Could not find default export for plugin "${pluginName}".`
                });
                continue;
            }
            if (!(pluginCtor instanceof Function)) {
                result.diagnostics.push({
                    pluginName,
                    message: `Default export did not have a constructor for plugin "${pluginName}".`
                });
                continue;
            }

            result.plugins.push(new pluginCtor());
        }
        catch (err) {
            result.diagnostics.push({
                pluginName,
                message: `Error loading plugin "${pluginName}": ${err}`
            });
        }
    }

    return result;
}
