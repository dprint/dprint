export function getFileExtension(filePath: string) {
    const lastPeriodIndex = filePath.lastIndexOf(".");
    if (lastPeriodIndex === -1)
        return "";

    return filePath.substring(lastPeriodIndex);
}
