// eslint-disable-next-line import/no-webpack-loader-syntax
import createWorker from "workerize-loader!./formatter.worker";

import type { FileMatchingInfo, PluginInfo } from "@dprint/formatter";

const formatterWorker = createWorker();
const pluginInfoListeners: ((info: PluginInfo) => void)[] = [];
const fileMatchingListeners: ((info: FileMatchingInfo) => void)[] = [];
const formatListeners: ((text: string) => void)[] = [];
const errorListeners: ((err: string) => void)[] = [];

formatterWorker.addEventListener("message", ev => {
  switch (ev.data.type) {
    case "PluginInfo":
      for (const listener of pluginInfoListeners) {
        listener(ev.data.info);
      }
      break;
    case "FileMatching":
      for (const listener of fileMatchingListeners) {
        listener(ev.data.info);
      }
      break;
    case "Format":
      for (const listener of formatListeners) {
        listener(ev.data.text);
      }
      break;
    case "Error":
      for (const listener of errorListeners) {
        listener(ev.data.message);
      }
      break;
  }
});

export function loadUrl(url: string) {
  formatterWorker.postMessage({
    type: "LoadUrl",
    url,
  });
}

export function setConfig(config: any) {
  formatterWorker.postMessage({
    type: "SetConfig",
    config,
  });
}

export function formatText(filePath: string, fileText: string) {
  formatterWorker.postMessage({
    type: "Format",
    filePath,
    fileText,
  });
}

export function addOnPluginInfo(listener: (info: PluginInfo) => void) {
  pluginInfoListeners.push(listener);
}

export function removeOnPluginInfo(listener: (info: PluginInfo) => void) {
  const index = pluginInfoListeners.indexOf(listener);
  if (index >= 0) {
    pluginInfoListeners.splice(index, 1);
  }
}

export function addOnFileMatchingInfo(listener: (info: FileMatchingInfo) => void) {
  fileMatchingListeners.push(listener);
}

export function removeOnFileMatchingInfo(listener: (info: FileMatchingInfo) => void) {
  const index = fileMatchingListeners.indexOf(listener);
  if (index >= 0) {
    fileMatchingListeners.splice(index, 1);
  }
}

export function addOnFormat(listener: (text: string) => void) {
  formatListeners.push(listener);
}

export function removeOnFormat(listener: (text: string) => void) {
  const index = formatListeners.indexOf(listener);
  if (index >= 0) {
    formatListeners.splice(index, 1);
  }
}

export function addOnError(listener: (err: string) => void) {
  errorListeners.push(listener);
}

export function removeOnError(listener: (err: string) => void) {
  const index = errorListeners.indexOf(listener);
  if (index >= 0) {
    errorListeners.splice(index, 1);
  }
}
