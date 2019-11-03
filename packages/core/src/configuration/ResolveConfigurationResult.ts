import { BaseResolvedConfiguration, ConfigurationDiagnostic  } from "@dprint/types";

/** The result of resolving configuration. */
export interface ResolveConfigurationResult<ResolvedConfiguration extends BaseResolvedConfiguration> {
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
    /** The resolved configuration. */
    config: ResolvedConfiguration;
}
