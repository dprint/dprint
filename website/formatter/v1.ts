// Copyright 2020-2021 by David Sherret. All rights reserved.
// This work is licensed under the terms of the MIT license.
// For a copy, see <https://opensource.org/licenses/MIT>.

console.warn(
  "[dprint]: Apologies for the warn. This module will be deprecated and 404 starting September 2021. "
    + "Please upgrade to the immutable deno.land url. For more details see: https://github.com/dprint/js-formatter",
);

/** Formats code. */
export interface Formatter {
  /**
   * Sets the configuration.
   * @param globalConfig - Global configuration for use across plugins.
   * @param pluginConfig - Plugin specific configuration.
   */
  setConfig(globalConfig: GlobalConfiguration, pluginConfig: object): void;
  /**
   * Gets the configuration diagnostics.
   */
  getConfigDiagnostics(): ConfigurationDiagnostic[];
  /**
   * Gets the resolved configuration.
   * @returns An object containing the resolved configuration.
   */
  getResolvedConfig(): object;
  /**
   * Gets the plugin info.
   */
  getPluginInfo(): PluginInfo;
  /**
   * Gets the license text of the plugin.
   */
  getLicenseText(): string;
  /**
   * Formats the specified file text.
   * @param filePath - The file path to format.
   * @param fileText - File text to format.
   * @returns The formatted text.
   * @throws If there is an error formatting.
   */
  formatText(filePath: string, fileText: string): string;
}

/** Configuration specified for use across plugins. */
export interface GlobalConfiguration {
  lineWidth?: number;
  indentWidth?: number;
  useTabs?: boolean;
  newLineKind?: "auto" | "lf" | "crlf" | "system";
}

/** A diagnostic indicating a problem with the specified configuration. */
export interface ConfigurationDiagnostic {
  propertyName: string;
  message: string;
}

/** Information about a plugin. */
export interface PluginInfo {
  name: string;
  version: string;
  configKey: string;
  fileExtensions: string[];
  helpUrl: string;
  configSchemaUrl: string;
}

/**
 * Creates the WebAssembly import object, if necessary.
 */
export function createImportObject(): any /*: WebAssembly.Imports*/ {
  // for now, use an identity object
  return {
    dprint: {
      "host_clear_bytes": () => {},
      "host_read_buffer": () => {},
      "host_write_buffer": () => {},
      "host_take_file_path": () => {},
      "host_format": () => 0, // no change
      "host_get_formatted_text": () => 0, // zero length
      "host_get_error_text": () => 0, // zero length
    },
  };
}

/**
 * Creates a formatter from the specified streaming source.
 * @remarks This is the most efficient way to create a formatter.
 * @param response - The streaming source to create the formatter from.
 */
export function createStreaming(response: Promise<Response>): Promise<Formatter> {
  if (WebAssembly.instantiateStreaming == null || typeof globalThis?.Deno != null) {
    return getArrayBuffer()
      .then(buffer => createFromBuffer(buffer));
  } else {
    return WebAssembly.instantiateStreaming(response, createImportObject())
      .then(obj => createFromInstance(obj.instance));
  }

  function getArrayBuffer() {
    if (isResponse(response)) {
      return response.arrayBuffer();
    } else {
      return response.then(response => response.arrayBuffer()) as Promise<ArrayBuffer>;
    }

    function isResponse(response: unknown): response is Response {
      return (response as Response).arrayBuffer != null;
    }
  }
}

/**
 * Creates a formatter from the specified wasm module bytes.
 * @param wasmModuleBuffer - The buffer of the wasm module.
 */
export function createFromBuffer(wasmModuleBuffer: BufferSource): Formatter {
  const wasmModule = new WebAssembly.Module(wasmModuleBuffer);
  const wasmInstance = new WebAssembly.Instance(wasmModule, createImportObject());
  return createFromInstance(wasmInstance);
}

/**
 * Creates a formatter from the specified wasm instance.
 * @param wasmInstance - The WebAssembly instance.
 */
export function createFromInstance(wasmInstance: WebAssembly.Instance): Formatter {
  const {
    get_plugin_schema_version,
    set_file_path,
    get_formatted_text,
    format,
    get_error_text,
    get_plugin_info,
    get_resolved_config,
    get_config_diagnostics,
    set_global_config,
    set_plugin_config,
    get_license_text,
    get_wasm_memory_buffer,
    get_wasm_memory_buffer_size,
    add_to_shared_bytes_from_buffer,
    set_buffer_with_shared_bytes,
    clear_shared_bytes,
    reset_config,
  } = wasmInstance.exports as any;

  const pluginSchemaVersion = get_plugin_schema_version();
  const expectedPluginSchemaVersion = 1;
  if (pluginSchemaVersion !== expectedPluginSchemaVersion) {
    throw new Error(
      `Not compatible plugin. `
        + `Expected schema ${expectedPluginSchemaVersion}, `
        + `but plugin had ${pluginSchemaVersion}.`,
    );
  }

  const bufferSize = get_wasm_memory_buffer_size();
  let configSet = false;

  return {
    setConfig(globalConfig, pluginConfig) {
      setConfig(globalConfig, pluginConfig);
    },
    getConfigDiagnostics() {
      setConfigIfNotSet();
      const length = get_config_diagnostics();
      return JSON.parse(receiveString(length));
    },
    getResolvedConfig() {
      setConfigIfNotSet();
      const length = get_resolved_config();
      return JSON.parse(receiveString(length));
    },
    getPluginInfo() {
      const length = get_plugin_info();
      return JSON.parse(receiveString(length));
    },
    getLicenseText() {
      const length = get_license_text();
      return receiveString(length);
    },
    formatText(filePath, fileText) {
      setConfigIfNotSet();
      sendString(filePath);
      set_file_path();

      sendString(fileText);
      const responseCode = format();
      switch (responseCode) {
        case 0: // no change
          return fileText;
        case 1: // change
          return receiveString(get_formatted_text());
        case 2: // error
          throw new Error(receiveString(get_error_text()));
        default:
          throw new Error(`Unexpected response code: ${responseCode}`);
      }
    },
  };

  function setConfigIfNotSet() {
    if (!configSet) {
      setConfig({}, {});
    }
  }

  function setConfig(globalConfig: object, pluginConfig: object) {
    if (reset_config != null) {
      reset_config();
    }
    sendString(JSON.stringify(globalConfig));
    set_global_config();
    sendString(JSON.stringify(getPluginConfigWithStringProps()));
    set_plugin_config();
    configSet = true;

    function getPluginConfigWithStringProps() {
      // Need to convert all the properties to strings so
      // they can be deserialized to a HashMap<String, String>.
      const newPluginConfig: { [key: string]: string } = {};
      for (const key of Object.keys(pluginConfig)) {
        newPluginConfig[key] = (pluginConfig as any)[key].toString();
      }
      return newPluginConfig;
    }
  }

  function sendString(text: string) {
    const encoder = new TextEncoder();
    const encodedText = encoder.encode(text);
    const length = encodedText.length;

    clear_shared_bytes(length);

    let index = 0;
    while (index < length) {
      const writeCount = Math.min(length - index, bufferSize);
      const wasmBuffer = getWasmBuffer(writeCount);
      for (let i = 0; i < writeCount; i++) {
        wasmBuffer[i] = encodedText[index + i];
      }
      add_to_shared_bytes_from_buffer(writeCount);
      index += writeCount;
    }
  }

  function receiveString(length: number) {
    const buffer = new Uint8Array(length);
    let index = 0;
    while (index < length) {
      const readCount = Math.min(length - index, bufferSize);
      set_buffer_with_shared_bytes(index, readCount);
      const wasmBuffer = getWasmBuffer(readCount);
      for (let i = 0; i < readCount; i++) {
        buffer[index + i] = wasmBuffer[i];
      }
      index += readCount;
    }
    const decoder = new TextDecoder();
    return decoder.decode(buffer);
  }

  function getWasmBuffer(length: number) {
    const pointer = get_wasm_memory_buffer();
    return new Uint8Array((wasmInstance.exports.memory as any).buffer, pointer, length);
  }
}
