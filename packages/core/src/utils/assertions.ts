export function throwError(message: string): never {
    throw getError(message);
}

export function getError(message: string): Error {
    return new Error(`[dprint]: ${message}`);
}
