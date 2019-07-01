import { Project } from "ts-morph";

export function createProject() {
    const project = new Project({
        tsConfigFilePath: "tsconfig.json"
    });

    return project;
}