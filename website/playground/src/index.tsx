import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom";
import { Spinner } from "./components";
import * as formatterWorker from "./FormatterWorker";
import "./index.css";
import { Playground } from "./Playground";
import { getPluginDefaultConfig, getPluginInfo, PluginInfo } from "./plugins";
import { UrlSaver } from "./utils";

const urlSaver = new UrlSaver();
const initialUrl = urlSaver.getUrlInfo();
let isFirstLoad = true;

function Loader() {
    const [plugins, setPlugins] = useState<PluginInfo[]>([]);
    const [plugin, setPlugin] = useState<PluginInfo | undefined>();
    const [fileExtensions, setFileExtensions] = useState<string[]>([]);
    const [text, setText] = useState(initialUrl.text);
    const [configText, setConfigText] = useState(initialUrl.configText ?? "");
    const [defaultConfigText, setDefaultConfigText] = useState("");
    const [formattedText, setFormattedText] = useState("");
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        getPluginInfo().then(plugins => {
            setPlugins(plugins);
            setPlugin(plugins.find(p => p.language === initialUrl.language ?? "typescript")!);
        }).catch(err => {
            console.error(err);
            alert("There was an error getting the plugins. Try refreshing the page or check the browser console.");
        });
    }, []);
    useEffect(() => {
        formatterWorker.addOnFormat(text => {
            setFormattedText(text);
        });

        formatterWorker.addOnError(err => {
            console.error(err);
            alert("There was an error with the formatter worker. Try refreshing the page or check the browser console.");
        });
    }, []);

    useEffect(() => {
        if (plugin == null) {
            return;
        }

        urlSaver.updateUrl({
            text,
            configText: configText === defaultConfigText ? undefined : configText,
            language: plugin.language,
        });
    }, [text, configText, plugin, defaultConfigText]);

    useEffect(() => {
        setIsLoading(true);

        if (plugin == null) {
            return;
        }

        const defaultConfigPromise = getPluginDefaultConfig(plugin);

        formatterWorker.loadUrl(plugin.url);

        defaultConfigPromise.then(defaultConfigText => {
            setFileExtensions([...plugin.fileExtensions]); // todo: get this from the wasm file (easy to do)

            if (isFirstLoad && initialUrl.configText != null) {
                setConfigText(initialUrl.configText);
                isFirstLoad = false;
            } else {
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

    if (plugin == null) {
        return <Spinner />;
    }

    return <Playground
        text={text}
        onTextChanged={setText}
        configText={configText}
        onConfigTextChanged={setConfigText}
        formattedText={formattedText}
        fileExtensions={fileExtensions}
        plugins={plugins}
        selectedPlugin={plugin}
        onSelectPlugin={setPlugin}
        isLoading={isLoading}
    />;
}

ReactDOM.render(<Loader />, document.getElementById("root"));
