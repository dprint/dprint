import { Environment } from "../../environment";

/**
 * A file writer that's resilient to the process being killed.
 *
 * This class writes to a temporary file then does a file system move.
 *
 * Doing many file writes in parallel is dangerous because if
 * the user manually kills the process then it will possibly make
 * many files zero length.
 */
export class KillSafeFileWriter {
    private tempFiles = new Set<string>();
    private crashed = false;

    constructor(private readonly environment: Environment) {
        // catch ctrl+c (see https://stackoverflow.com/a/14032965/188246)
        process.on("SIGINT", this.crashCleanup);
        // catch "kill pid"
        process.on("SIGUSR1", this.crashCleanup);
        process.on("SIGUSR2", this.crashCleanup);
    }

    dispose() {
        process.off("SIGINT", this.crashCleanup);
        process.off("SIGUSR1", this.crashCleanup);
        process.off("SIGUSR2", this.crashCleanup);
    }

    async writeFile(filePath: string, fileText: string): Promise<void> {
        // get a temporary file path
        let tempFilePath = this.getTempFileName(filePath);
        this.tempFiles.add(tempFilePath);
        // write to the temporary file
        await this.environment.writeFile(tempFilePath, fileText);
        // in case it already did the crash cleanup and this code is running for some reason
        if (this.crashed) {
            this.tryDeleteFileSync(tempFilePath);
            return;
        }

        // move the temporary file to the new location
        await this.environment.rename(tempFilePath, filePath);
        this.tempFiles.delete(tempFilePath);
    }

    private crashCleanup = () => {
        this.crashed = true;

        // Everything in here needs to be synchronous because this is
        // happening when the process is being killed.
        for (const filePath of this.tempFiles.values())
            this.tryDeleteFileSync(filePath);
    };

    private tryDeleteFileSync(filePath: string) {
        try {
            this.environment.unlinkSync(filePath);
        } catch {
            // ignore
        }
    }

    private getTempFileName(filePath: string) {
        // good enough
        return filePath + ".dprint_temp";
    }
}
