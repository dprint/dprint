import { Configuration, ConfigurationDiagnostic } from "@dprint/types";

const projectTypeInfo = {
    values: [{
        name: "openSource",
        description: "Dprint is formatting an open source project."
    }, {
        name: "commercialSponsored",
        description: "Dprint is formatting a closed source commercial project and your company sponsored dprint."
    }, {
        name: "commercialDidNotSponsor",
        description: "Dprint is formatting a closed source commercial project and you want to forever enshrine your name "
            + "in source control for having specified this."
    }]
};

/**
 * Checks if the configuration has a missing "projectType" property.
 * @param config Configuration.
 * @remarks This is done to encourage companies to support the project. They obviously aren't required to though.
 * Please discuss this with me if you have strong reservations about this. Note that this library took a lot of
 * time, effort, and previous built up knowledge and I'm happy to give it away for free to open source projects,
 * but would like to see companies support it financially even if it's only in a small way.
 */
export function getMissingProjectTypeDiagnostic(config: Configuration): ConfigurationDiagnostic | undefined {
    const validProjectTypes = projectTypeInfo.values.map(v => v.name);

    if (validProjectTypes.includes(config.projectType || ""))
        return undefined;

    const propertyName: keyof Configuration = "projectType";
    const largestValueName = validProjectTypes.map(s => s.length).sort().pop()!;

    return {
        propertyName,
        message: `The "${propertyName}" field is missing. You may specify any of the following possible values in the configuration file according to your `
            + `conscience and that will supress this warning.\n\n`
            + projectTypeInfo.values.map(value => ` * ${value.name} ${" ".repeat(largestValueName - value.name.length)}${value.description}`).join("\n")
    };
}
