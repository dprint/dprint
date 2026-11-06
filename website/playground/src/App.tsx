import type { FileMatchingInfo, PluginInfo } from "@dprint/formatter";
import { useEffect, useRef, useState } from "react";
import { Spinner } from "./components";
import * as formatterWorker from "./FormatterWorker";
import { Playground } from "./Playground";
import { getHostPluginUrls, getPluginSchemaDefaults, getPluginShortNameFromPluginUrl, getPluginUrls, type PlaygroundSample } from "./plugins";
import { UrlSaver } from "./utils";

const urlSaver = new UrlSaver();
const initialUrl = urlSaver.getUrlInfo();
let isFirstLoad = true;

export default function App() {
  const [pluginUrls, setPluginUrls] = useState<string[]>([]);
  const [pluginUrl, setPluginUrl] = useState(initialUrl.pluginUrl);
  const [pluginInfo, setPluginInfo] = useState<PluginInfo | undefined>();
  const [fileMatchingInfo, setFileMatchingInfo] = useState<FileMatchingInfo | undefined>();
  const [fileExtension, setFileExtension] = useState(initialUrl.ext ?? "");
  const [text, setText] = useState(initialUrl.text);
  const [configText, setConfigText] = useState(initialUrl.configText ?? "");
  const [defaultConfigText, setDefaultConfigText] = useState("");
  const [formattedText, setFormattedText] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  // the selected plugin's sample code, which is replaced when another plugin is
  // selected as long as it wasn't changed
  const sampleRef = useRef<PlaygroundSample | undefined>(undefined);
  const textRef = useRef(text);
  textRef.current = text;

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
    formatterWorker.addOnFileMatchingInfo(onFileMatchingInfo);
    formatterWorker.addOnFormat(onFormat);
    formatterWorker.addOnError(onError);

    return () => {
      formatterWorker.removeOnPluginInfo(onPluginInfo);
      formatterWorker.removeOnFileMatchingInfo(onFileMatchingInfo);
      formatterWorker.removeOnError(onError);
      formatterWorker.removeOnFormat(onFormat);
    };

    function onPluginInfo(pluginInfo: PluginInfo) {
      setPluginInfo(pluginInfo);
    }

    function onFileMatchingInfo(info: FileMatchingInfo) {
      setFileMatchingInfo(info);
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
      // the sample isn't in the url, so it's only shared once it's changed
      text: text === sampleRef.current?.text ? "" : text,
      configText: configText === defaultConfigText ? undefined : configText,
      plugin: isBuiltInLanguage ? shortName : pluginUrl,
      ext: fileMatchingInfo?.fileExtensions.indexOf(fileExtension) === 0 ? undefined : fileExtension,
    });
  }, [text, configText, pluginUrl, defaultConfigText, fileExtension, fileMatchingInfo]);

  // the plugins that format the code the selected plugin embeds
  const hostPluginUrls = pluginUrl == null ? "" : getHostPluginUrls(pluginUrl, pluginUrls).join(" ");
  useEffect(() => {
    setIsLoading(true);

    if (pluginUrl == null) {
      return;
    }

    formatterWorker.loadUrl(pluginUrl, hostPluginUrls === "" ? [] : hostPluginUrls.split(" "));
  }, [pluginUrl, hostPluginUrls]);

  useEffect(() => {
    if (pluginUrl == null || pluginInfo == null) {
      return;
    }

    const abortController = new AbortController();
    getPluginSchemaDefaults(pluginInfo.configSchemaUrl, abortController.signal).then(({ configText: defaultConfigText, sample }) => {
      if (isFirstLoad && initialUrl.configText != null) {
        setConfigText(initialUrl.configText);
        isFirstLoad = false;
      } else {
        setConfigText(defaultConfigText);
      }
      setDefaultConfigText(defaultConfigText);
      // starts with the plugin's sample when there's no code or the code is the
      // previous plugin's sample, which wasn't changed
      const previousSample = sampleRef.current;
      sampleRef.current = sample;
      if (textRef.current === "" || textRef.current === previousSample?.text) {
        setText(sample?.text ?? "");
        if (sample?.extension != null) {
          setFileExtension(sample.extension);
        }
      }
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

  if (pluginUrl == null || pluginInfo == null || fileMatchingInfo == null) {
    return <Spinner />;
  }

  return (
    <Playground
      text={text}
      onTextChanged={setText}
      configText={configText}
      onConfigTextChanged={setConfigText}
      fileExtension={fileExtension}
      onFileExtensionChanged={setFileExtension}
      formattedText={formattedText}
      pluginUrls={pluginUrls}
      selectedPluginUrl={pluginUrl}
      selectedPluginInfo={pluginInfo}
      fileMatchingInfo={fileMatchingInfo}
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
