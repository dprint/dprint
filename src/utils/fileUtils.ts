import * as fs from "fs";

export async function readFile(filePath: string) {
    return new Promise<string>((resolve, reject) => {
        fs.readFile(filePath, { encoding: "utf8" }, (err, text) => {
            if (err)
                reject(err);
            else
                resolve(text);
        });
    });
}

export async function writeFile(filePath: string, text: string) {
    return new Promise<void>((resolve, reject) => {
        fs.writeFile(filePath, text, { encoding: "utf8" }, (err) => {
            if (err)
                reject(err);
            else
                resolve();
        });
    });
}
