import { ResolvedGlobalConfiguration } from "./GlobalConfiguration";
import { ConfigurationDiagnostic } from "./ConfigurationDiagnostic";

/** The result of resolving configuration. */
export interface ResolveConfigurationResult<ResolvedConfiguration extends ResolvedGlobalConfiguration> {
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
    /** The resolved configuration. */
    config: ResolvedConfiguration;
}
