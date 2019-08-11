/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: string;
    /** The diagnostic's message that should be displayed to the user. */
    message: string;
}
