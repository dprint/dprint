export function assertNever(value: never): never {
    return throwError(`Unhandled value: ${JSON.stringify(value)}`);
}

export function throwError(message: string): never {
    throw new Error(`[dprint]: ${message}`);
}