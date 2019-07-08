import "@babel/preset-typescript";

export { formatFileText } from "./formatFileText";
export {
    Configuration,
    ConfigurationDiagnostic,
    resolveConfiguration,
    ResolveConfigurationResult,
    ResolvedConfiguration
} from "./configuration";
export {
    CliEnvironment,
    Environment,
    runCli
} from "./cli/index";