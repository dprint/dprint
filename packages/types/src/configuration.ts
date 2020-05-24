import { Plugin } from "./plugins";

/**
 * Dprint's configuration.
 */
export interface Configuration {
    /**
     * Specify the type of project this is. You may specify any of the allowed values here according to your conscience.
     * @value "openSource" - Dprint is formatting an open source project.
     * @value "commercialSponsored" - Dprint is formatting a commercial project and your company sponsored dprint.
     * @value "commercialDidNotSponsor" - Dprint is formatting a commercial project and you want to forever enshrine your name in source control for having specified this.
     */
    projectType: "openSource" | "commercialSponsored" | "commercialDidNotSponsor";
    /**
     * The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
     * @default 120
     */
    lineWidth?: number;
    /**
     * The number of characters for an indent.
     * @default 4
     */
    indentWidth?: number;
    /**
     * Whether to use tabs (true) or spaces (false).
     * @default false
     */
    useTabs?: boolean;
    /**
     * The kind of newline to use.
     * @default "lf"
     * @value "auto" - For each file, uses the newline kind found at the end of the last line.
     * @value "crlf" - Uses carriage return, line feed.
     * @value "lf" - Uses line feed.
     * @value "system" - Uses the system standard (ex. crlf on Windows).
     */
    newLineKind?: "auto" | "crlf" | "lf" | "system";
    /**
     * Collection of plugins to use.
     */
    plugins: Plugin[];
}

export interface ResolvedConfiguration extends BaseResolvedConfiguration {
}

export interface BaseResolvedConfiguration {
    readonly lineWidth: number;
    readonly indentWidth: number;
    readonly useTabs: boolean;
    readonly newLineKind: "auto" | "crlf" | "lf";
}

/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: string;
    /** The diagnostic's message that should be displayed to the user. */
    message: string;
}
