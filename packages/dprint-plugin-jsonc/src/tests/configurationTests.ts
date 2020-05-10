import { expect } from "chai";
import { Configuration as GlobalConfiguration, ConfigurationDiagnostic } from "@dprint/types";
import { resolveConfiguration as resolveGlobalConfiguration, CliLoggingEnvironment } from "@dprint/core";
import { JsoncConfiguration, ResolvedJsoncConfiguration } from "../Configuration";
import { JsoncPlugin } from "../Plugin";

describe("configuration", () => {
    function doTest(
        config: JsoncConfiguration,
        expectedConfig: Partial<ResolvedJsoncConfiguration>,
        propertyFilter: (propName: keyof ResolvedJsoncConfiguration) => boolean,
        expectedDiagnostics: ConfigurationDiagnostic[] = [],
        globalConfig: Partial<GlobalConfiguration> = {},
    ) {
        const resolvedGlobalConfig = resolveGlobalConfiguration(globalConfig).config;
        const resolvedConfigResult = resolveConfiguration();
        const resolvedConfig = resolvedConfigResult.config;

        for (const propName in resolvedConfig) {
            if (!propertyFilter(propName as keyof ResolvedJsoncConfiguration))
                delete (resolvedConfig as any)[propName];
        }

        expect(resolvedConfig).to.deep.equal(expectedConfig);
        expect(resolvedConfigResult.diagnostics).to.deep.equal(expectedDiagnostics);

        function resolveConfiguration() {
            const plugin = new JsoncPlugin(config);
            try {
                plugin.initialize({
                    environment: new CliLoggingEnvironment(),
                    globalConfig: resolvedGlobalConfig,
                });
                return {
                    config: plugin.getConfiguration(),
                    diagnostics: plugin.getConfigurationDiagnostics(),
                };
            } finally {
                plugin.dispose();
            }
        }
    }

    describe("diagnostics", () => {
        it("should do a diagnostic when providing an incorrect number value", () => {
            doTest({ lineWidth: false as any as number }, {}, () => false, [{
                message: "Error parsing configuration value for 'lineWidth'. Message: invalid digit found in string",
                propertyName: "lineWidth",
            }]);
        });

        it("should do a diagnostic when providing an incorrect boolean value", () => {
            doTest({ "commentLine.forceSpaceAfterSlashes": 5 as any as boolean }, {}, () => false, [{
                message: "Error parsing configuration value for 'commentLine.forceSpaceAfterSlashes'. Message: provided string was not `true` or `false`",
                propertyName: "commentLine.forceSpaceAfterSlashes",
            }]);
        });

        it("should do a diagnostic when providing an excess property", () => {
            doTest({ asdf: 5 } as any, {}, () => false, [{
                message: "Unknown property in configuration: asdf",
                propertyName: "asdf",
            }]);
        });
    });

    describe(nameof<JsoncConfiguration>(c => c["commentLine.forceSpaceAfterSlashes"]), () => {
        function doSpecificTest(config: JsoncConfiguration, expectedConfig: Partial<ResolvedJsoncConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "commentLine.forceSpaceAfterSlashes");
        }

        it("should get the default property", () => {
            doSpecificTest({}, { "commentLine.forceSpaceAfterSlashes": true });
        });

        it("should get the property when set", () => {
            doSpecificTest(
                { "commentLine.forceSpaceAfterSlashes": true },
                { "commentLine.forceSpaceAfterSlashes": true },
            );
        });
    });
});
