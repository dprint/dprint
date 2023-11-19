import type { PluginInfo } from "@dprint/formatter";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom";
import { Spinner } from "./components";
import * as formatterWorker from "./FormatterWorker";
import "./index.css";
import { Playground } from "./Playground";
import { getPluginDefaultConfig, getPluginShortNameFromPluginUrl, getPluginUrls } from "./plugins";
import { UrlSaver } from "./utils";

const urlSaver = new UrlSaver();
const initialUrl = urlSaver.getUrlInfo();
let isFirstLoad = true;

function Loader() {
  const [pluginUrls, setPluginUrls] = useState<string[]>([]);
  const [pluginUrl, setPluginUrl] = useState(initialUrl.pluginUrl);
  const [pluginInfo, setPluginInfo] = useState<PluginInfo | undefined>();
  const [text, setText] = useState(initialUrl.text);
  const [configText, setConfigText] = useState(initialUrl.configText ?? "");
  const [defaultConfigText, setDefaultConfigText] = useState("");
  const [formattedText, setFormattedText] = useState("");
  const [isLoading, setIsLoading] = useState(true);

  // initialization
  useEffect(() => {
    const abortController = new AbortController();
    getPluginUrls(abortController.signal).then(pluginUrls => {
      setPluginUrls(pluginUrls.concat(initialUrl.pluginUrl ?? []));
      if (initialUrl.pluginUrl == null) {
        setPluginUrl(pluginUrls.find(url => getPluginShortNameFromPluginUrl(url) === (initialUrl.pluginName ?? "typescript"))!);
      }
    }).catch(err => {
      if (!abortController.signal.aborted) {
        console.error(err);
        alert("There was an error getting the plugins. Try refreshing the page or check the browser console.");
      }
    });
    return () => {
      abortController.abort();
    };
  }, []);

  useEffect(() => {
    formatterWorker.addOnPluginInfo(onPluginInfo);
    formatterWorker.addOnFormat(onFormat);
    formatterWorker.addOnError(onError);

    return () => {
      formatterWorker.removeOnPluginInfo(onPluginInfo);
      formatterWorker.removeOnError(onError);
      formatterWorker.removeOnFormat(onFormat);
    };

    function onPluginInfo(pluginInfo: PluginInfo) {
      setPluginInfo(pluginInfo);
    }

    function onFormat(text: string) {
      setFormattedText(text);
    }

    function onError(err: string) {
      console.error(err);
      alert("There was an error with the formatter worker. Try refreshing the page or check the browser console.");
    }
  }, [setFormattedText, setPluginInfo]);

  useEffect(() => {
    if (pluginUrl == null) {
      return;
    }

    const shortName = getPluginShortNameFromPluginUrl(pluginUrl);
    const isBuiltInLanguage = !!shortName;

    urlSaver.updateUrl({
      text,
      configText: configText === defaultConfigText ? undefined : configText,
      plugin: isBuiltInLanguage ? shortName : pluginUrl,
    });
  }, [text, configText, pluginUrl, defaultConfigText]);

  useEffect(() => {
    setIsLoading(true);

    if (pluginUrl == null) {
      return;
    }

    formatterWorker.loadUrl(pluginUrl);
  }, [pluginUrl]);

  useEffect(() => {
    if (pluginUrl == null || pluginInfo == null) {
      return;
    }

    const abortController = new AbortController();
    getPluginDefaultConfig(pluginInfo.configSchemaUrl, abortController.signal).then(defaultConfigText => {
      if (isFirstLoad && initialUrl.configText != null) {
        setConfigText(initialUrl.configText);
        isFirstLoad = false;
      } else {
        setConfigText(defaultConfigText);
      }
      setDefaultConfigText(defaultConfigText);
      setIsLoading(false);
    }).catch(err => {
      if (abortController.signal.aborted) {
        return;
      }

      console.error(err);
      alert("There was an error loading the plugin. Check the console or try refreshing the page.");
    });

    return () => {
      abortController.abort();
    };
  }, [pluginUrl, pluginInfo]);

  if (pluginUrl == null || pluginInfo == null) {
    return <Spinner />;
  }

  return (
    <Playground
      text={text}
      onTextChanged={setText}
      configText={configText}
      onConfigTextChanged={setConfigText}
      formattedText={formattedText}
      pluginUrls={pluginUrls}
      selectedPluginUrl={pluginUrl}
      selectedPluginInfo={pluginInfo}
      onSelectPluginUrl={url => {
        setPluginInfo(undefined);
        if (!pluginUrls.includes(url)) {
          setPluginUrls([...pluginUrls, url]);
        }
        setPluginUrl(url);
      }}
      isLoading={isLoading}
    />
  );
}

ReactDOM.render(<Loader />, document.getElementById("root"));
