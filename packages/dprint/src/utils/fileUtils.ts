import * as fs from "fs";

export function readFile(filePath: string) {
    return new Promise<string>((resolve, reject) => {
        fs.readFile(filePath, { encoding: "utf8" }, (err, text) => {
            if (err)
                reject(err);
            else
                resolve(text);
        });
    });
}

export function writeFile(filePath: string, text: string) {
    return new Promise<void>((resolve, reject) => {
        fs.writeFile(filePath, text, { encoding: "utf8" }, err => {
            if (err)
                reject(err);
            else
                resolve();
        });
    });
}

export function rename(oldFilePath: string, newFilePath: string) {
    return new Promise<void>((resolve, reject) => {
        fs.rename(oldFilePath, newFilePath, err => {
            if (err)
                reject(err);
            else
                resolve();
        });
    });
}

export function exists(fileOrDirPath: string) {
    return new Promise<boolean>((resolve, reject) => {
        try {
            fs.exists(fileOrDirPath, result => {
                resolve(result);
            });
        } catch (err) {
            reject(err);
        }
    });
}
