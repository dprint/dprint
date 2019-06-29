import { expect } from "chai";
import * as os from "os";
import { Configuration, resolveConfiguration, ConfigurationDiagnostic, ResolvedConfiguration } from "../../configuration";

describe(nameof(resolveConfiguration), () => {
    function doTest(
        config: Configuration,
        expectedConfig: Partial<ResolvedConfiguration>,
        propertyFilter: (propName: keyof ResolvedConfiguration) => boolean,
        expectedDiagnostics: ConfigurationDiagnostic[] = []
    ) {
        const resolvedConfig = resolveConfiguration(config);
        for (const propName in resolvedConfig.config) {
            if (!propertyFilter(propName as keyof ResolvedConfiguration))
                delete (resolvedConfig.config as any)[propName];
        }

        expect(resolvedConfig.config).to.deep.equal(expectedConfig);
        expect(resolvedConfig.diagnostics).to.deep.equal(expectedDiagnostics);
    }

    describe("diagnostics", () => {
        it("should do a diagnostic when providing an incorrect boolean value", () => {
            doTest({ semiColons: 5 as any as boolean }, {}, () => false, [{
                message: "Expected the configuration for 'semiColons' to be a boolean, but its value was: 5",
                propertyName: "semiColons"
            }]);
        });

        it("should do a diagnostic when providing an excess property", () => {
            doTest({ asdf: 5 } as any, {}, () => false, [{
                message: "Unexpected property in configuration: asdf",
                propertyName: "asdf"
            }]);
        });
    });

    describe(nameof<Configuration>(c => c.semiColons), () => {
        function doSpecificTest(config: Configuration, expectedConfig: Partial<ResolvedConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("semiColon"));
        }

        it("should set all the semi-colon values using the default", () => {
            doSpecificTest({}, getObject(true));
        });

        it("should set all the semi-colon values when using the default", () => {
            doSpecificTest({ semiColons: true }, getObject(true));
        });

        it("should set all the semi-colon values when set to a non default", () => {
            doSpecificTest({ semiColons: false }, getObject(false));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(false);
            const config: Configuration = { ...expectedConfig };
            config.semiColons = true;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: boolean): Partial<ResolvedConfiguration> {
            return {
                "ifStatement.semiColon": value
            };
        }
    });

    describe(nameof<Configuration>(c => c.singleQuotes), () => {
        function doSpecificTest(config: Configuration, expectedConfig: Partial<ResolvedConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "singleQuotes");
        }

        it("should set when not set", () => {
            doSpecificTest({}, { singleQuotes: false });
        });

        it("should use when set to the default", () => {
            doSpecificTest({ singleQuotes: false }, { singleQuotes: false });
        });

        it("should use when not set to the default", () => {
            doSpecificTest({ singleQuotes: true }, { singleQuotes: true });
        });
    });

    describe(nameof<Configuration>(c => c.newLineKind), () => {
        function doSpecificTest(newLineKind: string | undefined, expectedKind: string) {
            doTest({ newLineKind: newLineKind as any }, { newLineKind: expectedKind as any }, prop => prop === "newLineKind");
        }

        it("should set when not set", () => {
            doSpecificTest(undefined, "auto");
        });

        it("should set when set to auto", () => {
            doSpecificTest("auto", "auto");
        });

        it("should set when set to crlf", () => {
            doSpecificTest("crlf", "crlf");
        });

        it("should set when set to lf", () => {
            doSpecificTest("lf", "lf");
        });

        it("should resolve when set to system", () => {
            doSpecificTest("system", os.EOL === "\r\n" ? "crlf" : "lf");
        });

        it("should do a diagnostic when providing an incorrect value", () => {
            doTest({ newLineKind: "asdf" as any }, {}, () => false, [{
                message: "Unknown configuration specified for 'newLineKind': asdf",
                propertyName: "newLineKind"
            }]);
        });
    });
});
