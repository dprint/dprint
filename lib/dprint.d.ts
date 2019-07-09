export declare function formatFileText(filePath: string, fileText: string, configuration: ResolvedConfiguration): string;

export interface Configuration {
    /**
     * The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
     * @default 120
     */
    lineWidth?: number;
    /**
     * The number of spaces for an indent. This option is ignored when using tabs.
     * @default 4
     */
    indentSize?: number;
    /**
     * Whether to use tabs (true) or spaces (false).
     * @default false
     */
    useTabs?: boolean;
    /**
     * Whether statements should use semi-colons.
     * @default true
     */
    semiColons?: boolean;
    /**
     * Whether to use single quotes (true) or double quotes (false).
     * @default false
     */
    singleQuotes?: boolean;
    /**
     * The kind of newline to use.
     * @default "auto"
     * @value "auto" - For each file, uses the newline kind found at the end of the last line.
     * @value "crlf" - Uses carriage return, line feed.
     * @value "lf" - Uses line feed.
     * @value "system" - Uses the system standard (ex. crlf on Windows).
     */
    newLineKind?: "auto" | "crlf" | "lf" | "system";
    /**
     * If braces should be used or not.
     * @default "maintain"
     * @value "maintain" - Uses braces if they're used. Doesn't use braces if they're not used.
     * @value "always" - Forces the use of braces. Will add them if they aren't used.
     * @value "preferNone" - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
     */
    useBraces?: "maintain" | "always" | "preferNone";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    bracePosition?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the next control flow within a control flow statement.
     * @default "nextLine"
     * @value "maintain" - Maintains the next control flow being on the next line or the same line.
     * @value "sameLine" - Forces the next control flow to be on the same line.
     * @value "nextLine" - Forces the next control flow to be on the next line.
     */
    nextControlFlowPosition?: "maintain" | "sameLine" | "nextLine";
    "directive.semiColon"?: boolean;
    "doWhileStatement.semiColon"?: boolean;
    "expressionStatement.semiColon"?: boolean;
    "ifStatement.semiColon"?: boolean;
    "importDeclaration.semiColon"?: boolean;
    "typeAlias.semiColon"?: boolean;
    /**
     * If braces should be used or not.
     * @default "maintain"
     * @value "maintain" - Uses braces if they're used. Doesn't use braces if they're not used.
     * @value "always" - Forces the use of braces. Will add them if they aren't used.
     * @value "preferNone" - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
     */
    "ifStatement.useBraces"?: "maintain" | "always" | "preferNone";
    /**
     * If braces should be used or not.
     * @default "maintain"
     * @value "maintain" - Uses braces if they're used. Doesn't use braces if they're not used.
     * @value "always" - Forces the use of braces. Will add them if they aren't used.
     * @value "preferNone" - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
     */
    "whileStatement.useBraces"?: "maintain" | "always" | "preferNone";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "classDeclaration.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "doWhileStatement.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "functionDeclaration.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "ifStatement.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "tryStatement.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    "whileStatement.bracePosition"?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the next control flow within a control flow statement.
     * @default "nextLine"
     * @value "maintain" - Maintains the next control flow being on the next line or the same line.
     * @value "sameLine" - Forces the next control flow to be on the same line.
     * @value "nextLine" - Forces the next control flow to be on the next line.
     */
    "ifStatement.nextControlFlowPosition"?: "maintain" | "sameLine" | "nextLine";
    /**
     * Where to place the next control flow within a control flow statement.
     * @default "nextLine"
     * @value "maintain" - Maintains the next control flow being on the next line or the same line.
     * @value "sameLine" - Forces the next control flow to be on the same line.
     * @value "nextLine" - Forces the next control flow to be on the next line.
     */
    "tryStatement.nextControlFlowPosition"?: "maintain" | "sameLine" | "nextLine";
}

export interface ConfigurationDiagnostic {
    propertyName: string;
    message: string;
}

export declare function resolveConfiguration(config: Configuration): ResolveConfigurationResult;

export interface ResolveConfigurationResult {
    /** The resolved configuration. */
    config: ResolvedConfiguration;
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
}

export interface ResolvedConfiguration {
    lineWidth: number;
    indentSize: number;
    useTabs: boolean;
    singleQuotes: boolean;
    newLineKind: "auto" | "\r\n" | "\n";
    "directive.semiColon": boolean;
    "doWhileStatement.semiColon": boolean;
    "expressionStatement.semiColon": boolean;
    "ifStatement.semiColon": boolean;
    "importDeclaration.semiColon": boolean;
    "typeAlias.semiColon": boolean;
    "ifStatement.useBraces": NonNullable<Configuration["useBraces"]>;
    "whileStatement.useBraces": NonNullable<Configuration["useBraces"]>;
    "classDeclaration.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "doWhileStatement.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "functionDeclaration.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "ifStatement.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "tryStatement.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "whileStatement.bracePosition": NonNullable<Configuration["bracePosition"]>;
    "ifStatement.nextControlFlowPosition": NonNullable<Configuration["nextControlFlowPosition"]>;
    "tryStatement.nextControlFlowPosition": NonNullable<Configuration["nextControlFlowPosition"]>;
}

export declare function runCli(args: string[], environment: Environment): Promise<void>;

export declare class RealEnvironment implements Environment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
    resolvePath(fileOrDirPath: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    glob(patterns: string[]): Promise<string[]>;
}

export interface Environment {
    log(text: string): void;
    warn(text: string): void;
    error(text: string): void;
    resolvePath(path: string): string;
    readFile(filePath: string): Promise<string>;
    writeFile(filePath: string, text: string): Promise<void>;
    glob(patterns: string[]): Promise<string[]>;
}
