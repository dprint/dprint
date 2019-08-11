import { BaseResolvedConfiguration } from "./Configuration";
import { ConfigurationDiagnostic } from "./ConfigurationDiagnostic";

/** The result of resolving configuration. */
export interface ResolveConfigurationResult<ResolvedConfiguration extends BaseResolvedConfiguration> {
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
    /** The resolved configuration. */
    config: ResolvedConfiguration;
}
