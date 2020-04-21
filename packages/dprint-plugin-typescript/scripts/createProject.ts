import { Project } from "ts-morph";

export function createProject() {
    return new Project({
        tsConfigFilePath: "tsconfig.json"
    });
}
