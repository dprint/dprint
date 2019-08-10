/** Represents an execution environment. */
export interface LoggingEnvironment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
}
