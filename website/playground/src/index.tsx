import React, { useState, useEffect } from "react";
import ReactDOM from "react-dom";
import "./index.css";
import { Playground } from "./Playground";
import * as serviceWorker from "./serviceWorker";
import { Formatter } from "./types";
import { Spinner } from "./components";
import { getPluginInfo, PluginInfo, getPluginDefaultConfig } from "./plugins";
import { UrlSaver } from "./utils";
import * as formatterModule from "./utils/formatter/v1"; // should be copied by post install script

const urlSaver = new UrlSaver();
const initialUrl = urlSaver.getUrlInfo();
let isFirstLoad = true;

function Loader() {
    const [plugins, setPlugins] = useState<PluginInfo[]>([]);
    const [plugin, setPlugin] = useState<PluginInfo | undefined>();
    const [formatter, setFormatter] = useState<Formatter | undefined>(undefined);
    const [text, setText] = useState(initialUrl.text);
    const [configText, setConfigText] = useState(initialUrl.configText ?? "");
    const [defaultConfigText, setDefaultConfigText] = useState("");
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        getPluginInfo().then(plugins => {
            setPlugins(plugins);
            setPlugin(plugins.find(p => p.language === initialUrl.language ?? "typescript")!);
        });
    }, []);

    useEffect(() => {
        if (formatter == null || plugin == null)
            return;

        urlSaver.updateUrl({
            text,
            configText: configText === defaultConfigText ? undefined : configText,
            language: plugin.language,
        });
    }, [formatter, text, configText, plugin, defaultConfigText]);

    useEffect(() => {
        setIsLoading(true);

        if (plugin == null)
            return;

        const defaultConfigPromise = getPluginDefaultConfig(plugin);

        Promise.all([formatterModule.createStreaming(fetch(plugin.url)), defaultConfigPromise])
            .then(([formatter, defaultConfigText]) => {
                const pluginInfo = formatter.getPluginInfo();
                const fileExtensions = [...pluginInfo.fileExtensions];
                let lastConfigText = "";

                setFormatter({
                    formatText(fileExtension: string, fileText) {
                        try {
                            return formatter.formatText("file." + fileExtension, fileText);
                        } catch (err) {
                            console.error(err);
                            return err.message;
                        }
                    },
                    setConfig(configText) {
                        if (lastConfigText === configText)
                            return;

                        let config;
                        try {
                            config = JSON.parse(configText);
                            if (config.lineWidth == null)
                                config.lineWidth = 80;
                        } catch (err) {
                            // ignore for now
                            return;
                        }
                        formatter.setConfig({}, config);
                        lastConfigText = configText;
                    },
                    getFileExtensions() {
                        return fileExtensions;
                    },
                    getConfigSchemaUrl() {
                        return pluginInfo.configSchemaUrl;
                    },
                });

                if (isFirstLoad && initialUrl.configText != null) {
                    setConfigText(initialUrl.configText);
                    isFirstLoad = false;
                }
                else {
                    setConfigText(defaultConfigText);
                }
                setDefaultConfigText(defaultConfigText);
                setIsLoading(false);
            })
            .catch(err => {
                console.error(err);
                alert("There was an error loading the plugin. Check the console or try refreshing the page.");
            });
    }, [plugin]);

    if (plugin == null)
        return <Spinner />;

    return <Playground
        formatter={formatter}
        text={text}
        onTextChanged={setText}
        configText={configText}
        onConfigTextChanged={setConfigText}
        plugins={plugins}
        selectedPlugin={plugin}
        onSelectPlugin={setPlugin}
        isLoading={isLoading}
    />;
}

ReactDOM.render(<Loader />, document.getElementById("root"));

serviceWorker.unregister();
