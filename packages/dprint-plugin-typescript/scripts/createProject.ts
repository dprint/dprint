import { Project, NewLineKind } from "ts-morph";

export function createProject() {
    const project = new Project({
        tsConfigFilePath: "tsconfig.json",
        manipulationSettings: {
            newLineKind: NewLineKind.CarriageReturnLineFeed,
        },
    });

    return project;
}
