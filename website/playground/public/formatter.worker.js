let formatter;
let config;
let nextFormat;

onmessage = function(e) {
    switch (e.data.type) {
        case "LoadUrl": {
            loadUrl(e.data.url);
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

function loadUrl(url) {
    formatter = fetch(url)
        .then(response => response.arrayBuffer())
        .then(wasmModuleBuffer => {
            const newFormatter = createFromBuffer(wasmModuleBuffer);

            if (config) {
                setConfigSync(newFormatter, config);
            }

            if (nextFormat) {
                formatSync(newFormatter, nextFormat.filePath, nextFormat.fileText);
            }

            return newFormatter;
        });

    formatter.catch(err => postError(err));
}

function setConfig(config) {
    config = config;

    if (formatter) {
        formatter.then(f => {
            setConfigSync(f, config);
            formatSync(f, nextFormat.filePath, nextFormat.fileText);
        });
    }
}

function format(filePath, fileText) {
    nextFormat = { filePath, fileText };

    if (formatter) {
        formatter.then(f => formatSync(f, filePath, fileText));
    }
}

function setConfigSync(f, config) {
    doHandlingError(() => f.setConfig({}, config));
}

function formatSync(f, filePath, fileText) {
    let result;
    try {
        result = f.formatText(filePath, fileText);
    } catch (err) {
        result = err.message;
    }
    postMessage({
        type: "Format",
        text: result,
    });
}

function doHandlingError(action) {
    try {
        action();
    } catch (err) {
        postError(err);
    }
}

function postError(err) {
    postMessage({
        type: "Error",
        message: err.message,
    });
}

// TODO: DON'T COPY AND PASTE THIS HERE

/**
 * Creates a formatter from the specified wasm module bytes.
 * @param {BufferSource} wasmModuleBuffer - The buffer of the wasm module.
 */
function createFromBuffer(wasmModuleBuffer) {
    const wasmModule = new WebAssembly.Module(wasmModuleBuffer);
    const wasmInstance = new WebAssembly.Instance(wasmModule);
    return createFromInstance(wasmInstance);
}

/**
 * Creates a formatter from the specified wasm instance.
 * @params {WebAssembly.Instance} The web assembly instance.
 */
function createFromInstance(wasmInstance) {
    const {
        set_file_path,
        get_formatted_text,
        format,
        get_error_text,
        get_plugin_info,
        get_resolved_config,
        get_config_diagnostics,
        set_global_config,
        set_plugin_config,
        get_plugin_schema_version,
        get_wasm_memory_buffer,
        get_wasm_memory_buffer_size,
        add_to_shared_bytes_from_buffer,
        set_buffer_with_shared_bytes,
        clear_shared_bytes,
        reset_config,
    } = wasmInstance.exports;

    const pluginSchemaVersion = get_plugin_schema_version();
    const expectedPluginSchemaVersion = 1;
    if (pluginSchemaVersion !== expectedPluginSchemaVersion) {
        throw new Error(`Not compatible plugin. Expected schema ${expectedPluginSchemaVersion}, but plugin had ${pluginSchemaVersion}.`);
    }

    const bufferSize = get_wasm_memory_buffer_size();
    let configSet = false;

    return {
        /**
         * Sets the configuration.
         * @param {{
         *  lineWidth?: number;
         *  indentWidth?: number;
         *  useTabs?: boolean;
         *  newLineKind?: "auto" | "lf" | "crlf" | "system";
         * }} globalConfig - Global configuration.
         * @param {object} pluginConfig - Plugin configuration.
         */
        setConfig(globalConfig, pluginConfig) {
            setConfig(globalConfig, pluginConfig);
        },
        /**
         * Gets the configuration diagnostics.
         * @returns {{ propertyName: string; message: string; }[]} The configuration diagnostics.
         */
        getConfigDiagnostics() {
            setConfigIfNotSet();
            const length = get_config_diagnostics();
            return JSON.parse(receiveString(length));
        },
        /**
         * Gets the resolved configuration.
         * @returns {object} An object containing the resolved configuration.
         */
        getResolvedConfig() {
            setConfigIfNotSet();
            const length = get_resolved_config();
            return JSON.parse(receiveString(length));
        },
        /**
         * Gets the plugin info.
         * @returns {{
         *  name: string;
         *  version: string;
         *  configKey: string;
         *  fileExtensions: string[];
         *  helpUrl: string;
         *  configSchemaUrl: string;
         * }} The plugin info.
         */
        getPluginInfo() {
            const length = get_plugin_info();
            return JSON.parse(receiveString(length));
        },
        /**
         *
         * @param {string} filePath - The file path to format.
         * @param {string} fileText - File text to format.
         * @returns {string} The formatted text.
         * @throws If there is an error formatting.
         */
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

    function setConfig(globalConfig, pluginConfig) {
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
            // they will be deserialized to a HashMap<String, String>.
            const newPluginConfig = {};
            for (const key of Object.keys(pluginConfig)) {
                newPluginConfig[key] = pluginConfig[key].toString();
            }
            return newPluginConfig;
        }
    }

    /** @param {string} text */
    function sendString(text) {
        const encoder = new TextEncoder();
        const encodedText = encoder.encode(text);
        const length = encodedText.length;

        clear_shared_bytes(length);

        let index = 0;
        while (index < length) {
            const writeCount = Math.min(length - index, bufferSize);
            const pointer = get_wasm_memory_buffer();
            const wasmBuffer = new Uint8Array(wasmInstance.exports.memory.buffer, pointer, writeCount);
            for (let i = 0; i < writeCount; i++) {
                wasmBuffer[i] = encodedText[index + i];
            }
            add_to_shared_bytes_from_buffer(writeCount);
            index += writeCount;
        }
    }

    /** @param {number} length */
    function receiveString(length) {
        const buffer = new Uint8Array(length);
        let index = 0;
        while (index < length) {
            const readCount = Math.min(length - index, bufferSize);
            set_buffer_with_shared_bytes(index, readCount);
            const pointer = get_wasm_memory_buffer();
            const wasmBuffer = new Uint8Array(wasmInstance.exports.memory.buffer, pointer, readCount);
            for (let i = 0; i < readCount; i++) {
                buffer[index + i] = wasmBuffer[i];
            }
            index += readCount;
        }
        const decoder = new TextDecoder();
        return decoder.decode(buffer);
    }
}
