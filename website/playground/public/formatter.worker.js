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
    // This special download route will load the plugins from the plugins.dprint.dev server
    // to allow CORs instead of doing a redirect to GitHub, which won't allow CORs.
    url = url.replace("https://plugins.dprint.dev/", "https://plugins.dprint.dev/download/");

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
            if (nextFormat) {
                formatSync(f, nextFormat.filePath, nextFormat.fileText);
            }
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
 * Creates the WebAssembly import object, if necessary.
 */
function createImportObject() {
    // for now, use an identity object
    return {
        dprint: {
            "host_clear_bytes": function() {},
            "host_read_buffer": function() {},
            "host_write_buffer": function() {},
            "host_take_file_path": function() {},
            "host_take_override_config": function() {},
            "host_format": function() {
                return 0;
            },
            "host_get_formatted_text": function() {
                return 0;
            },
            "host_get_error_text": function() {
                return 0;
            },
        },
    };
}
/**
 * Creates a formatter from the specified wasm module bytes.
 * @param wasmModuleBuffer - The buffer of the wasm module.
 */
function createFromBuffer(wasmModuleBuffer) {
    var wasmModule = new WebAssembly.Module(wasmModuleBuffer);
    var wasmInstance = new WebAssembly.Instance(wasmModule, createImportObject());
    return createFromInstance(wasmInstance);
}
/**
 * Creates a formatter from the specified wasm instance.
 * @param wasmInstance - The WebAssembly instance.
 */
function createFromInstance(wasmInstance) {
    var _a = wasmInstance.exports,
        get_plugin_schema_version = _a.get_plugin_schema_version,
        set_file_path = _a.set_file_path,
        set_override_config = _a.set_override_config,
        get_formatted_text = _a.get_formatted_text,
        format = _a.format,
        get_error_text = _a.get_error_text,
        get_plugin_info = _a.get_plugin_info,
        get_resolved_config = _a.get_resolved_config,
        get_config_diagnostics = _a.get_config_diagnostics,
        set_global_config = _a.set_global_config,
        set_plugin_config = _a.set_plugin_config,
        get_license_text = _a.get_license_text,
        get_wasm_memory_buffer = _a.get_wasm_memory_buffer,
        get_wasm_memory_buffer_size = _a.get_wasm_memory_buffer_size,
        add_to_shared_bytes_from_buffer = _a.add_to_shared_bytes_from_buffer,
        set_buffer_with_shared_bytes = _a.set_buffer_with_shared_bytes,
        clear_shared_bytes = _a.clear_shared_bytes,
        reset_config = _a.reset_config;
    var pluginSchemaVersion = get_plugin_schema_version();
    var expectedPluginSchemaVersion = 3;
    if (pluginSchemaVersion !== 2 && pluginSchemaVersion !== expectedPluginSchemaVersion) {
        throw new Error(
            "Not compatible plugin. "
                + ("Expected schema " + expectedPluginSchemaVersion + ", ")
                + ("but plugin had " + pluginSchemaVersion + "."),
        );
    }
    var bufferSize = get_wasm_memory_buffer_size();
    var configSet = false;
    return {
        setConfig: function(globalConfig, pluginConfig) {
            setConfig(globalConfig, pluginConfig);
        },
        getConfigDiagnostics: function() {
            setConfigIfNotSet();
            var length = get_config_diagnostics();
            return JSON.parse(receiveString(length));
        },
        getResolvedConfig: function() {
            setConfigIfNotSet();
            var length = get_resolved_config();
            return JSON.parse(receiveString(length));
        },
        getPluginInfo: function() {
            var length = get_plugin_info();
            return JSON.parse(receiveString(length));
        },
        getLicenseText: function() {
            var length = get_license_text();
            return receiveString(length);
        },
        formatText: function(filePath, fileText, overrideConfig) {
            setConfigIfNotSet();
            if (overrideConfig != null) {
                if (pluginSchemaVersion === 2) {
                    throw new Error("Cannot set the override configuration for this old plugin.");
                }
                sendString(JSON.stringify(overrideConfig));
                set_override_config();
            }
            sendString(filePath);
            set_file_path();
            sendString(fileText);
            var responseCode = format();
            switch (responseCode) {
                case 0: // no change
                    return fileText;
                case 1: // change
                    return receiveString(get_formatted_text());
                case 2: // error
                    throw new Error(receiveString(get_error_text()));
                default:
                    throw new Error("Unexpected response code: " + responseCode);
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
        sendString(JSON.stringify(pluginConfig));
        set_plugin_config();
        configSet = true;
    }
    function sendString(text) {
        var encoder = new TextEncoder();
        var encodedText = encoder.encode(text);
        var length = encodedText.length;
        clear_shared_bytes(length);
        var index = 0;
        while (index < length) {
            var writeCount = Math.min(length - index, bufferSize);
            var wasmBuffer = getWasmBuffer(writeCount);
            for (var i = 0; i < writeCount; i++) {
                wasmBuffer[i] = encodedText[index + i];
            }
            add_to_shared_bytes_from_buffer(writeCount);
            index += writeCount;
        }
    }
    function receiveString(length) {
        var buffer = new Uint8Array(length);
        var index = 0;
        while (index < length) {
            var readCount = Math.min(length - index, bufferSize);
            set_buffer_with_shared_bytes(index, readCount);
            var wasmBuffer = getWasmBuffer(readCount);
            for (var i = 0; i < readCount; i++) {
                buffer[index + i] = wasmBuffer[i];
            }
            index += readCount;
        }
        var decoder = new TextDecoder();
        return decoder.decode(buffer);
    }
    function getWasmBuffer(length) {
        var pointer = get_wasm_memory_buffer();
        return new Uint8Array(wasmInstance.exports.memory.buffer, pointer, length);
    }
}
