/// <reference lib="webworker" />
import { createFromBuffer, Formatter } from "@dprint/formatter";
import { formatTextUntilStable } from "./stableFormat";

let formatter: Promise<Formatter> | undefined;
let config: Record<string, unknown> | undefined;
let nextFormat: { filePath: string; fileText: string } | undefined;
let abortController = new AbortController();

onmessage = function(e) {
  switch (e.data.type) {
    case "LoadUrl": {
      loadUrl(e.data.url, e.data.hostUrls ?? []);
      break;
    }
    case "SetConfig": {
      setConfig(e.data.config);
      break;
    }
    case "Format": {
      format(e.data.filePath, e.data.fileText);
      break;
    }
  }
};

function loadUrl(url: string, hostUrls: string[]) {
  abortController.abort();
  abortController = new AbortController();
  const signal = abortController.signal;
  const fetchBuffer = (url: string) => fetch(url, { signal }).then(response => response.arrayBuffer());
  formatter = Promise.all([fetchBuffer(url), ...hostUrls.map(fetchBuffer)])
    .then(([wasmModuleBuffer, ...hostBuffers]) => {
      const newFormatter = createFromBuffer(wasmModuleBuffer);
      setHostFormatters(newFormatter, hostBuffers.map(buffer => createFromBuffer(buffer)));

      postPluginInfo(newFormatter);

      setConfigSync(newFormatter, config ?? {});

      if (nextFormat) {
        formatSync(newFormatter, nextFormat.filePath, nextFormat.fileText);
      }

      return newFormatter;
    });

  formatter.catch((err: any) => {
    if (!signal.aborted) {
      postError(err);
    }
  });
}

// formats the code a plugin embeds (ex. the cells of a notebook) with the
// plugin that handles its file extension, or leaves it as it is when none does
function setHostFormatters(f: Formatter, hostFormatters: Formatter[]) {
  if (hostFormatters.length === 0) {
    return;
  }
  const hosts = hostFormatters.map(hostFormatter => {
    hostFormatter.setConfig({}, {});
    return { formatter: hostFormatter, fileMatching: hostFormatter.getFileMatchingInfo() };
  });
  f.setHostFormatter(request => {
    const fileName = request.filePath.split(/[\/]/).pop() ?? "";
    const extension = fileName.includes(".") ? fileName.slice(fileName.lastIndexOf(".") + 1).toLowerCase() : undefined;
    const host = hosts.find(host =>
      (extension != null && host.fileMatching.fileExtensions.some(ext => ext.toLowerCase() === extension))
      || host.fileMatching.fileNames.includes(fileName)
    );
    if (host == null) {
      return request.fileText;
    }
    try {
      return host.formatter.formatText({ filePath: request.filePath, fileText: request.fileText });
    } catch {
      // like the CLI, code that doesn't parse is left as it is
      return request.fileText;
    }
  });
}

function setConfig(providedConfig: Record<string, unknown>) {
  config = providedConfig;
  refresh();
}

function refresh() {
  if (formatter) {
    formatter.then(f => {
      setConfigSync(f, config ?? {});
      if (nextFormat) {
        formatSync(f, nextFormat.filePath, nextFormat.fileText);
      }
    });
  }
}

function format(filePath: string, fileText: string) {
  nextFormat = { filePath, fileText };

  if (formatter) {
    formatter.then(f => formatSync(f, filePath, fileText));
  }
}

function setConfigSync(f: Formatter, config: Record<string, unknown>) {
  doHandlingError(() => f.setConfig({}, config));
  postMessage({
    type: "FileMatching",
    info: f.getFileMatchingInfo(),
  });
}

function formatSync(f: Formatter, filePath: string, fileText: string) {
  let result;
  try {
    result = formatTextUntilStable(fileText, fileText => f.formatText({ filePath, fileText }));
  } catch (err: any) {
    result = err.message;
  }
  postMessage({
    type: "Format",
    text: result,
  });
}

function postPluginInfo(f: Formatter) {
  postMessage({
    type: "PluginInfo",
    info: f.getPluginInfo(),
  });
}

function doHandlingError(action: () => void) {
  try {
    action();
  } catch (err: any) {
    postError(err);
  }
}

function postError(err: Error) {
  postMessage({
    type: "Error",
    message: err.message,
  });
}
