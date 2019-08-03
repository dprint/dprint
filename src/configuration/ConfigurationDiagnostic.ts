import { Configuration } from "./Configuration";

/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: keyof Configuration;
    /** The diagnostic's message. */
    message: string;
}
