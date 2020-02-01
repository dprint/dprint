import { expect } from "chai";
import * as os from "os";
import { Configuration, resolveConfiguration, ConfigurationDiagnostic, ResolvedConfiguration } from "../../configuration";

describe(nameof(resolveConfiguration), () => {
    function doTest(
        config: Partial<Configuration>,
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
        it("should do a diagnostic when providing an incorrect number value", () => {
            doTest({ lineWidth: false as any as number }, {}, () => false, [{
                message: "Expected the configuration for 'lineWidth' to be a number, but its value was: false",
                propertyName: "lineWidth"
            }]);
        });

        it("should do a diagnostic when providing an incorrect boolean value", () => {
            doTest({ useTabs: 5 as any as boolean }, {}, () => false, [{
                message: "Expected the configuration for 'useTabs' to be a boolean, but its value was: 5",
                propertyName: "useTabs"
            }]);
        });

        it("should do a diagnostic when providing an excess property", () => {
            doTest({ asdf: 5 } as any, {}, () => false, [{
                message: "Unknown property in configuration: asdf",
                propertyName: "asdf"
            }]);
        });
    });

    describe("defaults", () => {
        it("should get the defaults", () => {
            doTest({}, {
                indentWidth: 4,
                lineWidth: 120,
                newLineKind: "auto",
                useTabs: false
            }, () => true);
        });

        it("should set when not using the defaults", () => {
            doTest({
                indentWidth: 2,
                lineWidth: 80,
                newLineKind: "crlf",
                useTabs: true
            }, {
                indentWidth: 2,
                lineWidth: 80,
                newLineKind: "\r\n",
                useTabs: true
            }, () => true);
        });
    });

    describe(nameof<Configuration>(c => c.newLineKind), () => {
        function doSpecificTest(value: string | undefined, expectedValue: string) {
            doTest({ newLineKind: value as any }, { newLineKind: expectedValue as any }, prop => prop === "newLineKind");
        }

        it("should set when not set", () => {
            doSpecificTest(undefined, "auto");
        });

        it("should set when set to auto", () => {
            doSpecificTest("auto", "auto");
        });

        it("should set when set to crlf", () => {
            doSpecificTest("crlf", "\r\n");
        });

        it("should set when set to lf", () => {
            doSpecificTest("lf", "\n");
        });

        it("should resolve when set to system", () => {
            doSpecificTest("system", os.EOL === "\r\n" ? "\r\n" : "\n");
        });

        it("should do a diagnostic when providing an incorrect value", () => {
            doTest({ newLineKind: "asdf" as any }, {}, () => false, [{
                message: "Unknown configuration specified for 'newLineKind': asdf",
                propertyName: "newLineKind"
            }]);
        });
    });
});
