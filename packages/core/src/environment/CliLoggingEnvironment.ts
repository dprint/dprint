import { LoggingEnvironment } from "./LoggingEnvironment";

/**
 * An implementation of an environment that outputs to the console.
 */
export class CliLoggingEnvironment implements LoggingEnvironment {
    log(text: string) {
        console.log(text);
    }

    warn(text: string) {
        console.warn(text);
    }

    error(text: string) {
        console.error(text);
    }
}
