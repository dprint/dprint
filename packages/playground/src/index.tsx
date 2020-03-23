import React from "react";
import ReactDOM from "react-dom";
import "./index.css";
import { Playground } from "./Playground";
import { Spinner } from "./components";
import * as serviceWorker from "./serviceWorker";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "dprint-plugin-typescript";

interface LoaderState {
    formatText: ((text: string, configuration: TypeScriptConfiguration) => string) | undefined;
    resolveConfig: ((configuration: TypeScriptConfiguration) => ResolvedTypeScriptConfiguration) | undefined;
}

class Loader extends React.Component<{}, LoaderState> {
    constructor(props: {}) {
        super(props);

        this.state = {
            formatText: undefined,
            resolveConfig: undefined,
        };

        import("./wasm").then(wasmPkg => {
            this.setState({
                formatText: (text, config) => {
                    return wasmPkg.format_text(text, getConfigAsMap(config));
                },
                resolveConfig: config => {
                    return JSON.parse(wasmPkg.resolve_config(getConfigAsMap(config))) as ResolvedTypeScriptConfiguration;
                },
            });
        }).catch(console.error);
    }

    render() {
        if (this.state.formatText == null || this.state.resolveConfig == null)
            return <Spinner />;
        else
            return <Playground formatText={this.state.formatText} resolveConfig={this.state.resolveConfig} />;
    }
}

ReactDOM.render(<Loader />, document.getElementById("root"));

serviceWorker.unregister();

function getConfigAsMap(config: TypeScriptConfiguration) {
    const map = new Map();
    for (let key of Object.keys(config)) {
        const value = (config as any)[key] as unknown;
        if (value == null)
            continue;
        else if (typeof value === "string" || typeof value === "boolean" || typeof value === "number")
            map.set(key, value.toString());
        else
            throw new Error(`Not supported value type '${typeof value}' for key '${key}'.`);
    }
    return map;
}
