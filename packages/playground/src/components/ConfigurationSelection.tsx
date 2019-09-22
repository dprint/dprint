import React from "react";
import { AssertTrue, IsExact } from "conditional-type-checks";
import { TypeScriptConfiguration } from "dprint-plugin-typescript";
import "./ConfigurationSelection.css";

export interface ConfigurationSelectionProps {
    config: TypeScriptConfiguration;
    onUpdateConfig: (config: TypeScriptConfiguration) => void;
}

const useBraceOptions = ["maintain", "whenNotSingleLine", "always", "preferNone"] as const;
type _assertUseBraces = AssertTrue<IsExact<typeof useBraceOptions[number], NonNullable<TypeScriptConfiguration["useBraces"]>>>;
const bracePositionOptions = ["maintain", "sameLine", "nextLine", "nextLineIfHanging"] as const;
type _assertBracePosition = AssertTrue<IsExact<typeof bracePositionOptions[number], NonNullable<TypeScriptConfiguration["bracePosition"]>>>;
const singleBodyPositionOptions = ["maintain", "sameLine", "nextLine"] as const;
type _assertSingleBodyPositionOptions = AssertTrue<IsExact<typeof singleBodyPositionOptions[number],
    NonNullable<TypeScriptConfiguration["singleBodyPosition"]>>>;
const nextControlFlowPositionOptions = ["maintain", "sameLine", "nextLine"] as const;
type _assertNextControlFlowPosition = AssertTrue<IsExact<typeof nextControlFlowPositionOptions[number],
    NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>>>;
const operatorPositionOptions = ["maintain", "sameLine", "nextLine"] as const;
type _assertOperatorPosition = AssertTrue<IsExact<typeof operatorPositionOptions[number], NonNullable<TypeScriptConfiguration["operatorPosition"]>>>;
const trailingCommaOptions = ["never", "always", "onlyMultiLine"] as const;
type _assertTrailingCommas = AssertTrue<IsExact<typeof trailingCommaOptions[number], NonNullable<TypeScriptConfiguration["trailingCommas"]>>>;
const arrowFunctionUseParenthesesOptions = ["force", "maintain", "preferNone"] as const;
type _assertArrowFunctionUseParentheses = AssertTrue<IsExact<typeof arrowFunctionUseParenthesesOptions[number],
    NonNullable<TypeScriptConfiguration["arrowFunctionExpression.useParentheses"]>>>;
const enumMemberSpacingOptions = ["newline", "blankline", "maintain"] as const;
type _assertEnumMemberSpacing = AssertTrue<IsExact<typeof enumMemberSpacingOptions[number],
    NonNullable<TypeScriptConfiguration["enumDeclaration.memberSpacing"]>>>;

export class ConfigurationSelection extends React.Component<ConfigurationSelectionProps> {
    render() {
        return <div id="configuration">
            <ConfigurationItem title="Line width">
                {this.getNumberConfig("lineWidth")}
            </ConfigurationItem>
            <ConfigurationItem title="Indent width">
                {this.getNumberConfig("indentWidth")}
            </ConfigurationItem>
            <ConfigurationItem title="Use tabs">
                {this.getBooleanConfig("useTabs")}
            </ConfigurationItem>
            <ConfigurationItem title="Semicolons">
                {this.getBooleanConfig("semiColons")}
            </ConfigurationItem>
            <ConfigurationItem title="Single quotes">
                {this.getBooleanConfig("singleQuotes")}
            </ConfigurationItem>
            <ConfigurationItem title="Use braces">
                {this.getSelectForConfig("useBraces", useBraceOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Brace position">
                {this.getSelectForConfig("bracePosition", bracePositionOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Single body position">
                {this.getSelectForConfig("singleBodyPosition", singleBodyPositionOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Operator position">
                {this.getSelectForConfig("operatorPosition", operatorPositionOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Next control flow position">
                {this.getSelectForConfig("nextControlFlowPosition", nextControlFlowPositionOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Trailing commas">
                {this.getSelectForConfig("trailingCommas", trailingCommaOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Force multi-line arguments">
                {this.getBooleanConfig("forceMultiLineArguments")}
            </ConfigurationItem>
            <ConfigurationItem title="Force multi-line parameters">
                {this.getBooleanConfig("forceMultiLineParameters")}
            </ConfigurationItem>
            <ConfigurationItem title="Arrow Function - Use parentheses">
                {this.getSelectForConfig("arrowFunctionExpression.useParentheses", arrowFunctionUseParenthesesOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Enum member spacing">
                {this.getSelectForConfig("enumDeclaration.memberSpacing", enumMemberSpacingOptions)}
            </ConfigurationItem>
            <ConfigurationItem title="Use space separators">
                {this.getBooleanConfig("useSpaceSeparators")}
            </ConfigurationItem>
        </div>;
    }

    private getBooleanConfig(configKey: keyof TypeScriptConfiguration) {
        const { config } = this.props;
        return (
            <input type="checkbox" checked={config[configKey] as boolean} onChange={() => this.updateConfig({ [configKey]: !config[configKey] })} />
        );
    }

    private getSelectForConfig(configKey: keyof TypeScriptConfiguration, values: readonly string[]) {
        const { config } = this.props;
        return (
            <select value={config[configKey] as string} onChange={e => this.updateConfig({ [configKey]: e.target.value as any })}>
                {getOptionsForValues()}
            </select>
        );

        function getOptionsForValues() {
            return values.map((value, i) => (<option key={i} value={value}>{value}</option>));
        }
    }

    private getNumberConfig(configKey: keyof TypeScriptConfiguration) {
        const { config } = this.props;
        return (
            <input type="number" inputMode="numeric" value={config[configKey] as number} onChange={e => {
                const result = Math.max(0, Math.round(e.target.valueAsNumber));
                this.updateConfig({ [configKey]: result });
            }} />
        );
    }

    private updateConfig(newConfig: Partial<TypeScriptConfiguration>) {
        this.props.onUpdateConfig({ ...this.props.config, ...newConfig });
    }
}

interface ConfigurationItemProps {
    title: string;
}

class ConfigurationItem extends React.Component<ConfigurationItemProps> {
    render() {
        return (
            <div className="configurationItem">
                <label>
                    {this.props.title}:
                    {this.props.children}
                </label>
            </div>
        );
    }
}
