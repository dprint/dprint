import { expect } from "chai";
import { Configuration as GlobalConfiguration, ConfigurationDiagnostic } from "@dprint/types";
import { resolveConfiguration as resolveGlobalConfiguration, CliLoggingEnvironment } from "@dprint/core";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "../Configuration";
import { TypeScriptPlugin } from "../Plugin";

describe("configuration", () => {
    function doTest(
        config: TypeScriptConfiguration,
        expectedConfig: Partial<ResolvedTypeScriptConfiguration>,
        propertyFilter: (propName: keyof ResolvedTypeScriptConfiguration) => boolean,
        expectedDiagnostics: ConfigurationDiagnostic[] = [],
        globalConfig: Partial<GlobalConfiguration> = {},
    ) {
        const resolvedGlobalConfig = resolveGlobalConfiguration(globalConfig).config;
        const resolvedConfigResult = resolveConfiguration();
        const resolvedConfig = resolvedConfigResult.config;

        for (const propName in resolvedConfig) {
            if (!propertyFilter(propName as keyof ResolvedTypeScriptConfiguration))
                delete (resolvedConfig as any)[propName];
        }

        expect(resolvedConfig).to.deep.equal(expectedConfig);
        expect(resolvedConfigResult.diagnostics).to.deep.equal(expectedDiagnostics);

        function resolveConfiguration() {
            const plugin = new TypeScriptPlugin(config);
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
            doTest({ "setAccessor.spaceBeforeParentheses": 5 as any as boolean }, {}, () => false, [{
                message: "Error parsing configuration value for 'setAccessor.spaceBeforeParentheses'. Message: provided string was not `true` or `false`",
                propertyName: "setAccessor.spaceBeforeParentheses",
            }]);
        });

        it("should do a diagnostic when providing an excess property", () => {
            doTest({ asdf: 5 } as any, {}, () => false, [{
                message: "Unknown property in configuration: asdf",
                propertyName: "asdf",
            }]);
        });
    });

    describe(nameof<TypeScriptConfiguration>(c => c.semiColons), () => {
        function doSpecificTest(value: TypeScriptConfiguration["semiColons"] | undefined, expectedValue: ResolvedTypeScriptConfiguration["semiColons"]) {
            doTest({ semiColons: value }, { semiColons: expectedValue }, prop => prop === "semiColons");
        }

        it("should set when not set", () => {
            doSpecificTest(undefined, "prefer");
        });

        it("should use when set to the default", () => {
            doSpecificTest("prefer", "prefer");
        });

        it("should use when not set to the default", () => {
            doSpecificTest("always", "always");
            doSpecificTest("asi", "asi");
        });
    });

    describe(nameof<TypeScriptConfiguration>(c => c.quoteStyle), () => {
        function doSpecificTest(value: TypeScriptConfiguration["quoteStyle"] | undefined, expectedValue: ResolvedTypeScriptConfiguration["quoteStyle"]) {
            doTest({ quoteStyle: value }, { quoteStyle: expectedValue }, prop => prop === "quoteStyle");
        }

        it("should set when not set", () => {
            doSpecificTest(undefined, "preferDouble");
        });

        it("should use when set to the default", () => {
            doSpecificTest("preferDouble", "preferDouble");
        });

        it("should use when not set to the default", () => {
            doSpecificTest("alwaysDouble", "alwaysDouble");
            doSpecificTest("alwaysSingle", "alwaysSingle");
            doSpecificTest("preferSingle", "preferSingle");
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
                "whileStatement.useBraces": value,
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
                "arrowFunction.bracePosition": value,
                "classDeclaration.bracePosition": value,
                "classExpression.bracePosition": value,
                "constructor.bracePosition": value,
                "doWhileStatement.bracePosition": value,
                "enumDeclaration.bracePosition": value,
                "forInStatement.bracePosition": value,
                "forOfStatement.bracePosition": value,
                "forStatement.bracePosition": value,
                "functionDeclaration.bracePosition": value,
                "functionExpression.bracePosition": value,
                "getAccessor.bracePosition": value,
                "ifStatement.bracePosition": value,
                "interfaceDeclaration.bracePosition": value,
                "method.bracePosition": value,
                "moduleDeclaration.bracePosition": value,
                "setAccessor.bracePosition": value,
                "switchStatement.bracePosition": value,
                "switchCase.bracePosition": value,
                "tryStatement.bracePosition": value,
                "whileStatement.bracePosition": value,
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
                "whileStatement.singleBodyPosition": value,
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.nextControlFlowPosition), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("nextControlFlowPosition"));
        }

        const defaultValue = "sameLine";
        const nonDefaultValue = "nextLine";

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(defaultValue));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ nextControlFlowPosition: defaultValue }, getObject(defaultValue));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ nextControlFlowPosition: nonDefaultValue }, getObject(nonDefaultValue));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(nonDefaultValue);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.nextControlFlowPosition = defaultValue;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "ifStatement.nextControlFlowPosition": value,
                "tryStatement.nextControlFlowPosition": value,
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
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.trailingCommas), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.endsWith("trailingCommas"));
        }

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject("onlyMultiLine"));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ trailingCommas: "onlyMultiLine" }, getObject("onlyMultiLine"));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ trailingCommas: "always" }, getObject("always"));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject("always");
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.trailingCommas = "onlyMultiLine";
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["trailingCommas"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arguments.trailingCommas": value,
                "parameters.trailingCommas": value,
                "arrayExpression.trailingCommas": value,
                "arrayPattern.trailingCommas": value,
                "enumDeclaration.trailingCommas": value,
                "exportDeclaration.trailingCommas": value,
                "importDeclaration.trailingCommas": value,
                "objectExpression.trailingCommas": value,
                "objectPattern.trailingCommas": value,
                "typeParameters.trailingCommas": value,
                "tupleType.trailingCommas": value,
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.preferHanging), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.includes("preferHanging"));
        }

        let defaultValue = false;

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(defaultValue));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ preferHanging: defaultValue }, getObject(defaultValue));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ preferHanging: !defaultValue }, getObject(!defaultValue));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(defaultValue);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.preferHanging = !defaultValue;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["preferHanging"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arguments.preferHanging": value,
                "arrayExpression.preferHanging": value,
                "arrayPattern.preferHanging": value,
                "doWhileStatement.preferHanging": value,
                "extendsClause.preferHanging": value,
                "implementsClause.preferHanging": value,
                "exportDeclaration.preferHanging": value,
                "forInStatement.preferHanging": value,
                "forOfStatement.preferHanging": value,
                "forStatement.preferHanging": value,
                "ifStatement.preferHanging": value,
                "importDeclaration.preferHanging": value,
                "objectExpression.preferHanging": value,
                "objectPattern.preferHanging": value,
                "parameters.preferHanging": value,
                "sequenceExpression.preferHanging": value,
                "switchStatement.preferHanging": value,
                "tupleType.preferHanging": value,
                "typeLiteral.preferHanging": value,
                "typeParameters.preferHanging": value,
                "unionAndIntersectionType.preferHanging": value,
                "variableStatement.preferHanging": value,
                "whileStatement.preferHanging": value,
            };
        }
    });

    describe(nameof<TypeScriptConfiguration>(c => c.preferSingleLine), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop.includes("preferSingleLine"));
        }

        let defaultValue = false;

        it("should set all the values using the default", () => {
            doSpecificTest({}, getObject(defaultValue));
        });

        it("should set all the values when using the default", () => {
            doSpecificTest({ preferSingleLine: defaultValue }, getObject(defaultValue));
        });

        it("should set all the values when set to a non-default", () => {
            doSpecificTest({ preferSingleLine: !defaultValue }, getObject(!defaultValue));
        });

        it("should allow setting specific values when not the default", () => {
            const expectedConfig = getObject(defaultValue);
            const config: TypeScriptConfiguration = { ...expectedConfig } as any;
            config.preferSingleLine = !defaultValue;
            doSpecificTest(config, expectedConfig);
        });

        function getObject(value: NonNullable<TypeScriptConfiguration["preferSingleLine"]>): Partial<ResolvedTypeScriptConfiguration> {
            return {
                "arrayExpression.preferSingleLine": value,
                "arrayPattern.preferSingleLine": value,
                "arguments.preferSingleLine": value,
                "conditionalExpression.preferSingleLine": value,
                "conditionalType.preferSingleLine": value,
                "exportDeclaration.preferSingleLine": value,
                "forStatement.preferSingleLine": value,
                "importDeclaration.preferSingleLine": value,
                "mappedType.preferSingleLine": value,
                "memberExpression.preferSingleLine": value,
                "objectExpression.preferSingleLine": value,
                "objectPattern.preferSingleLine": value,
                "parameters.preferSingleLine": value,
                "parentheses.preferSingleLine": value,
                "tupleType.preferSingleLine": value,
                "typeLiteral.preferSingleLine": value,
                "typeParameters.preferSingleLine": value,
                "unionAndIntersectionType.preferSingleLine": value,
                "variableStatement.preferSingleLine": value,
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
                { "enumDeclaration.memberSpacing": "blankline" },
            );
        });
    });

    describe("arrowFunction.useParentheses", () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "arrowFunction.useParentheses");
        }

        it("should get the default property", () => {
            doSpecificTest({}, { "arrowFunction.useParentheses": "maintain" });
        });

        it("should get the property when set", () => {
            doSpecificTest(
                { "arrowFunction.useParentheses": "preferNone" },
                { "arrowFunction.useParentheses": "preferNone" },
            );
        });
    });

    describe("memberExpression.maintainLineBreaks", () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => prop === "memberExpression.maintainLineBreaks");
        }

        it("should get the default property", () => {
            doSpecificTest({}, { "memberExpression.maintainLineBreaks": true });
        });

        it("should get the property when set", () => {
            doSpecificTest(
                { "memberExpression.maintainLineBreaks": false },
                { "memberExpression.maintainLineBreaks": false },
            );
        });
    });

    describe("space settings", () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => expectedConfig.hasOwnProperty(prop));
        }

        function createConfigWithValue(keys: (keyof TypeScriptConfiguration)[], value: boolean): TypeScriptConfiguration {
            const config: TypeScriptConfiguration = {};
            for (const key of keys)
                (config as any)[key] = value;
            return config;
        }

        it("should set the space settings", () => {
            const keys: (keyof TypeScriptConfiguration)[] = [
                "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator",
                "commentLine.forceSpaceAfterSlashes",
                "constructor.spaceBeforeParentheses",
                "constructorType.spaceAfterNewKeyword",
                "constructSignature.spaceAfterNewKeyword",
                "doWhileStatement.spaceAfterWhileKeyword",
                "exportDeclaration.spaceSurroundingNamedExports",
                "forInStatement.spaceAfterForKeyword",
                "forOfStatement.spaceAfterForKeyword",
                "forStatement.spaceAfterForKeyword",
                "forStatement.spaceAfterSemiColons",
                "functionDeclaration.spaceBeforeParentheses",
                "functionExpression.spaceBeforeParentheses",
                "functionExpression.spaceAfterFunctionKeyword",
                "getAccessor.spaceBeforeParentheses",
                "ifStatement.spaceAfterIfKeyword",
                "importDeclaration.spaceSurroundingNamedImports",
                "jsxExpressionContainer.spaceSurroundingExpression",
                "method.spaceBeforeParentheses",
                "setAccessor.spaceBeforeParentheses",
                "taggedTemplate.spaceBeforeLiteral",
                "typeAnnotation.spaceBeforeColon",
                "typeAssertion.spaceBeforeExpression",
                "whileStatement.spaceAfterWhileKeyword",
            ];

            doSpecificTest(createConfigWithValue(keys, true), createConfigWithValue(keys, true) as any);
            doSpecificTest(createConfigWithValue(keys, false), createConfigWithValue(keys, false) as any);
        });
    });

    describe(nameof<TypeScriptConfiguration>(c => c.deno), () => {
        function doSpecificTest(config: TypeScriptConfiguration, expectedConfig: Partial<ResolvedTypeScriptConfiguration>) {
            doTest(config, expectedConfig, prop => expectedConfig.hasOwnProperty(prop));
        }

        it("should set some of the configuration", () => {
            doSpecificTest({
                deno: true,
            }, {
                lineWidth: 80,
                indentWidth: 2,
                "ifStatement.nextControlFlowPosition": "sameLine",
                "ifStatement.bracePosition": "sameLine",
                "commentLine.forceSpaceAfterSlashes": false,
                "constructSignature.spaceAfterNewKeyword": true,
                "constructorType.spaceAfterNewKeyword": true,
                "arrowFunction.useParentheses": "force",
                "newLineKind": "lf",
                "functionExpression.spaceAfterFunctionKeyword": true,
                "taggedTemplate.spaceBeforeLiteral": false,
            });
        });
    });
});
