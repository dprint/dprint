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
