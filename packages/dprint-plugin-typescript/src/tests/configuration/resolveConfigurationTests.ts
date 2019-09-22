import { expect } from "chai";
import { Configuration as GlobalConfiguration, ConfigurationDiagnostic, resolveConfiguration as resolveGlobalConfiguration } from "@dprint/core";
import { TypeScriptConfiguration, resolveConfiguration, ResolvedTypeScriptConfiguration } from "../../configuration";

describe(nameof(resolveConfiguration), () => {
    function doTest(
        config: TypeScriptConfiguration,
        expectedConfig: Partial<ResolvedTypeScriptConfiguration>,
        propertyFilter: (propName: keyof ResolvedTypeScriptConfiguration) => boolean,
        expectedDiagnostics: ConfigurationDiagnostic[] = [],
        globalConfig: Partial<GlobalConfiguration> = {}
    ) {
        const resolvedGlobalConfig = resolveGlobalConfiguration(globalConfig).config;
        const resolvedConfigResult = resolveConfiguration(resolvedGlobalConfig, config);
        const resolvedConfig = {
            // make a copy because the object is frozen
            ...resolvedConfigResult.config
        };

        for (const propName in resolvedConfig) {
            if (!propertyFilter(propName as keyof ResolvedTypeScriptConfiguration))
                delete (resolvedConfig as any)[propName];
        }

        expect(resolvedConfig).to.deep.equal(expectedConfig);
        expect(resolvedConfigResult.diagnostics).to.deep.equal(expectedDiagnostics);
    }

    describe("diagnostics", () => {
        it("should do a diagnostic when providing an incorrect number value", () => {
            doTest({ lineWidth: false as any as number }, {}, () => false, [{
                message: "Expected the configuration for 'lineWidth' to be a number, but its value was: false",
                propertyName: "lineWidth"
            }]);
        });

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

    describe(nameof<TypeScriptConfiguration>(c => c.semiColons), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("semiColon"));
        }

        it("should set all the semi-colon values using the default", () => {
            doSpecificTest({}, getObject(true));
        });

        it("should set all the semi-colon values when using the default", () => {
            doSpecificTest({ semiColons: true }, getObject(true));
        });

        it("should set all the semi-colon values when set to a non-default", () => {
            doSpecificTest({ semiColons: false }, getObject(false));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(false);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.semiColons = true;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: boolean): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "breakStatement.semiColon": value,
                "callSignature.semiColon": value,
                "classMethod.semiColon": value,
                "classProperty.semiColon": value,
                "constructSignature.semiColon": value,
                "continueStatement.semiColon": value,
                "debuggerStatement.semiColon": value,
                "directive.semiColon": value,
                "doWhileStatement.semiColon": value,
                "exportAllDeclaration.semiColon": value,
                "exportAssignment.semiColon": value,
                "exportDefaultDeclaration.semiColon": value,
                "exportNamedDeclaration.semiColon": value,
                "expressionStatement.semiColon": value,
                "functionDeclaration.semiColon": value,
                "importDeclaration.semiColon": value,
                "importEqualsDeclaration.semiColon": value,
                "indexSignature.semiColon": value,
                "mappedType.semiColon": value,
                "methodSignature.semiColon": value,
                "moduleDeclaration.semiColon": value,
                "namespaceExportDeclaration.semiColon": value,
                "propertySignature.semiColon": value,
                "returnStatement.semiColon": value,
                "throwStatement.semiColon": value,
                "typeAlias.semiColon": value,
                "variableStatement.semiColon": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.singleQuotes), () => {
        function doSpecificTest(value: boolean | undefined, expectedValue: boolean) {
            doTest({ singleQuotes: value as any }, { singleQuotes: expectedValue as any }, prop => prop === "singleQuotes");
        }

        it("should set when not set", () => {
            doSpecificTest(undefined, false);
        });

        it("should use when set to the default", () => {
            doSpecificTest(true, true);
        });

        it("should use when not set to the default", () => {
            doSpecificTest(false, false);
        });
    });

    describe(nameof<TypeScriptConfiguration>(c => c.useBraces), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("useBraces"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("whenNotSingleLine"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ useBraces: "whenNotSingleLine" }, getObject("whenNotSingleLine"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ useBraces: "always" }, getObject("always"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("always");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.useBraces = "maintain";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["useBraces"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "forInStatement.useBraces": value,
                "forOfStatement.useBraces": value,
                "forStatement.useBraces": value,
                "ifStatement.useBraces": value,
                "whileStatement.useBraces": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.bracePosition), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("bracePosition"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("nextLineIfHanging"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ bracePosition: "nextLineIfHanging" }, getObject("nextLineIfHanging"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ bracePosition: "nextLine" }, getObject("nextLine"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("nextLine");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.bracePosition = "nextLineIfHanging";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["bracePosition"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arrowFunctionExpression.bracePosition": value,
                "classDeclaration.bracePosition": value,
                "classExpression.bracePosition": value,
                "classMethod.bracePosition": value,
                "doWhileStatement.bracePosition": value,
                "enumDeclaration.bracePosition": value,
                "forInStatement.bracePosition": value,
                "forOfStatement.bracePosition": value,
                "forStatement.bracePosition": value,
                "functionDeclaration.bracePosition": value,
                "functionExpression.bracePosition": value,
                "ifStatement.bracePosition": value,
                "interfaceDeclaration.bracePosition": value,
                "moduleDeclaration.bracePosition": value,
                "switchStatement.bracePosition": value,
                "switchCase.bracePosition": value,
                "tryStatement.bracePosition": value,
                "whileStatement.bracePosition": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.singleBodyPosition), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("singleBodyPosition"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("maintain"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ singleBodyPosition: "maintain" }, getObject("maintain"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ singleBodyPosition: "nextLine" }, getObject("nextLine"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("maintain");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.singleBodyPosition = "nextLine";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["singleBodyPosition"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "forInStatement.singleBodyPosition": value,
                "forOfStatement.singleBodyPosition": value,
                "forStatement.singleBodyPosition": value,
                "ifStatement.singleBodyPosition": value,
                "whileStatement.singleBodyPosition": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.nextControlFlowPosition), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("nextControlFlowPosition"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("nextLine"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ nextControlFlowPosition: "nextLine" }, getObject("nextLine"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ nextControlFlowPosition: "sameLine" }, getObject("sameLine"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("sameLine");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.nextControlFlowPosition = "nextLine";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "ifStatement.nextControlFlowPosition": value,
                "tryStatement.nextControlFlowPosition": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.operatorPosition), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("operatorPosition"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("nextLine"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ operatorPosition: "nextLine" }, getObject("nextLine"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ operatorPosition: "sameLine" }, getObject("sameLine"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("sameLine");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.operatorPosition = "nextLine";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["operatorPosition"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "binaryExpression.operatorPosition": value,
                "conditionalExpression.operatorPosition": value,
                "logicalExpression.operatorPosition": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.trailingCommas), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("trailingCommas"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("never"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ trailingCommas: "never" }, getObject("never"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ trailingCommas: "always" }, getObject("always"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("always");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.trailingCommas = "never";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["trailingCommas"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arrayExpression.trailingCommas": value,
                "arrayPattern.trailingCommas": value,
                "enumDeclaration.trailingCommas": value,
                "objectExpression.trailingCommas": value,
                "tupleType.trailingCommas": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.forceMultiLineArguments), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("forceMultiLineArguments"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(false));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ forceMultiLineArguments: false }, getObject(false));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ forceMultiLineArguments: true }, getObject(true));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(false);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.forceMultiLineArguments = true;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["forceMultiLineArguments"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "callExpression.forceMultiLineArguments": value,
                "newExpression.forceMultiLineArguments": value
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.forceMultiLineParameters), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("forceMultiLineParameters"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(false));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ forceMultiLineParameters: false }, getObject(false));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ forceMultiLineParameters: true }, getObject(true));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(false);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.forceMultiLineParameters = true;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arrowFunctionExpression.forceMultiLineParameters": value,
                "callSignature.forceMultiLineParameters": value,
                "classMethod.forceMultiLineParameters": value,
                "constructSignature.forceMultiLineParameters": value,
                "constructorType.forceMultiLineParameters": value,
                "functionDeclaration.forceMultiLineParameters": value,
                "functionExpression.forceMultiLineParameters": value,
                "functionType.forceMultiLineParameters": value,
                "methodSignature.forceMultiLineParameters": value,
                "objectMethod.forceMultiLineParameters": value
            };
        }
    });

    describe("enumDeclaration.memberSpacing", () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "enumDeclaration.memberSpacing");
        }

        it("should get the default property", () => {
            doSpecificTest({}, { "enumDeclaration.memberSpacing": "maintain" });
        });

        it("should get the property when set", () => {
            doSpecificTest(
                { "enumDeclaration.memberSpacing": "blankline" },
                { "enumDeclaration.memberSpacing": "blankline" }
            );
        });
    });

    describe("arrowFunctionExpression.useParentheses", () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "arrowFunctionExpression.useParentheses");
        }

        it("should get the default property", () => {
            doSpecificTest({}, { "arrowFunctionExpression.useParentheses": "maintain" });
        });

        it("should get the property when set", () => {
            doSpecificTest(
                { "arrowFunctionExpression.useParentheses": "preferNone" },
                { "arrowFunctionExpression.useParentheses": "preferNone" }
            );
        });
    });


    describe(nameof<TypeScriptConfiguration>(c => c.useSpaces), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("useSpace"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(true));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ useSpaces: true }, getObject(true));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ useSpaces: false }, getObject(false));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(false);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.useSpaces = true;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: boolean): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "constructorType.useSpace": value,
                "constructSignature.useSpace": value,
                "doWhileStatement.useSpace": value,
                "exportDeclaration.useSpace": value,
                "forInStatement.useSpace": value,
                "forOfStatement.useSpace": value,
                "forStatement.useSpace": value,
                "ifStatement.useSpace": value,
                "importDeclaration.useSpace": value,
                "typeAssertion.useSpace": value,
                "whileStatement.useSpace": value
            };
        }
    });
});
