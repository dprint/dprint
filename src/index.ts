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
    runCli
} from "./cli";
export {
    RealEnvironment as CliEnvironment,
    Environment
} from "./environment";
