import { CliLoggingEnvironment } from "@dprint/core";
import { LoggingEnvironment, Configuration as CoreConfiguration } from "@dprint/types";
/**
 * Function used by the cli to format files.
 * @param args - Command line arguments.
 * @param environment - Environment to run the cli in.
 */
export declare function runCli(args: string[], environment: Environment): Promise<void>;

/** Configuration for a dprint.config.js file. */
export interface Configuration extends CoreConfiguration {
    /** File globs of files to format. */
    includes?: string[];
    /** File globs of files to not format. */
    excludes?: string[];
}

/**
 * An implementation of an environment that interacts with the user's file system and outputs to the console.
 */
export declare class CliEnvironment extends CliLoggingEnvironment implements Environment {
    basename(fileOrDirPath: string): string;
    resolvePath(fileOrDirPath: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    exists(filePath: string): Promise<boolean>;
    glob(patterns: string[]): Promise<string[]>;
    require(filePath: string): Promise<unknown>;
    rename(oldFilePath: string, newFilePath: string): Promise<void>;
    unlinkSync(filePath: string): void;
}

/** Represents an execution environment. */
export interface Environment extends LoggingEnvironment {
    resolvePath(path: string): string;
    basename(filePath: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    exists(filePath: string): Promise<boolean>;
    glob(patterns: string[]): Promise<string[]>;
    require(path: string): Promise<unknown>;
    rename(oldFilePath: string, newFilePath: string): Promise<void>;
    unlinkSync(filePath: string): void;
}
