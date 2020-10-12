use crate::environment::Environment;
use dprint_core::configuration::ConfigurationDiagnostic;
use crate::utils::get_table_text;

/// Checks if the configuration has a missing "project type" property.
///
/// This is done to encourage companies to support the project. They obviously aren't required to though.
/// Please discuss this with me if you have strong reservations about this. Note that this application took a lot of
/// time, effort, and previous built up knowledge and I'm happy to give it away for free to open source projects,
/// but would like to see companies support it financially even if it's only in a small way.
pub fn handle_project_type_diagnostic(environment: &impl Environment, project_type: &Option<String>) -> Option<ConfigurationDiagnostic> {
    let property_name = "projectType";
    let project_type_infos = get_project_type_infos();
    let has_project_type_config = match project_type {
        Some(project_type) => project_type_infos.iter().any(|info| info.name.to_lowercase() == project_type.to_lowercase()),
        _ => false,
    };

    if has_project_type_config {
        None
    } else {
        Some(ConfigurationDiagnostic {
            property_name: String::from(property_name),
            message: build_message(environment, &project_type_infos, property_name),
        })
    }
}

fn build_message(environment: &impl Environment, project_type_infos: &Vec<ProjectTypeInfo>, property_name: &str) -> String {
    let mut message = String::new();
    message.push_str(&format!("The '{}' property is missing in the configuration file.\n\n", property_name));
    message.push_str("You may specify any of the following values and that will suppress this error:\n");
    let options = project_type_infos.iter().map(|info| (format!("* {}", info.name), info.description.to_string())).collect::<Vec<_>>();
    let option_texts = get_table_text(options.iter().map(|(name, description)| (name.as_str(), description.as_str())).collect());
    message.push_str("\n");
    message.push_str(&option_texts.render(/* indent */ 1, Some(environment.get_terminal_width())));
    message.push_str("\n\nMore information: https://dprint.dev/sponsor");
    message
}

pub struct ProjectTypeInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub fn get_project_type_infos() -> Vec<ProjectTypeInfo> {
    vec![ProjectTypeInfo {
        name: "commercialEvaluation",
        description: concat!(
            "Dprint is formatting a project whose primary maintainer is a for-profit ",
            "company or individual and it is being evaluated for 30 days.",
        )
    }, ProjectTypeInfo {
        name: "commercialSponsored",
        description: concat!(
            "Dprint is formatting a project whose primary maintainer is a for-profit ",
            "company or individual AND the primary maintainer sponsored the project. ",
            "Thank you for being part of moving this project forward!"
        ),
    }, ProjectTypeInfo {
        name: "openSource",
        description: concat!(
            "Dprint is formatting an open source project whose primary ",
            "maintainer is not a for-profit company (no sponsorship requirement)."
        )
    }, ProjectTypeInfo {
        name: "educational",
        description: concat!(
            "Dprint is formatting a project run by a student or being used for ",
            "educational purposes (no sponsorship requirement)."
        )
    }, ProjectTypeInfo {
        name: "nonProfit",
        description: concat!(
            "Dprint is formatting a project whose primary maintainer is a non-profit ",
            "organization (no sponsorship requirement)."
        )
    }]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use crate::environment::TestEnvironment;
    use super::handle_project_type_diagnostic;

    #[test]
    fn it_should_handle_when_project_type_exists() {
        let environment = TestEnvironment::new();
        let result = handle_project_type_diagnostic(&environment, &Some(String::from("openSource")));
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_be_case_insensitive() {
        // don't get people too upset :)
        let environment = TestEnvironment::new();
        let result = handle_project_type_diagnostic(&environment, &Some(String::from("opensource")));
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_handle_when_project_type_not_exists() {
        let environment = TestEnvironment::new();
        let result = handle_project_type_diagnostic(&environment, &None);
        assert_eq!(result.is_some(), true);
        let result = result.unwrap();
        assert_eq!(result.property_name, "projectType");
        assert_eq!(result.message, r#"The 'projectType' property is missing in the configuration file.

You may specify any of the following values and that will suppress this error:

 * commercialEvaluation Dprint is formatting a project whose
                        primary maintainer is a for-profit
                        company or individual and it is
                        being evaluated for 30 days.
 * commercialSponsored  Dprint is formatting a project whose
                        primary maintainer is a for-profit
                        company or individual AND the
                        primary maintainer sponsored the
                        project. Thank you for being part of
                        moving this project forward!
 * openSource           Dprint is formatting an open source
                        project whose primary maintainer is
                        not a for-profit company (no
                        sponsorship requirement).
 * educational          Dprint is formatting a project run
                        by a student or being used for
                        educational purposes (no sponsorship
                        requirement).
 * nonProfit            Dprint is formatting a project whose
                        primary maintainer is a non-profit
                        organization (no sponsorship
                        requirement).

More information: https://dprint.dev/sponsor"#);
    }

    #[test]
    fn it_should_handle_when_project_type_not_valid_option() {
        let environment = TestEnvironment::new();
        let result = handle_project_type_diagnostic(&environment, &Some(String::from("test")));
        assert_eq!(result.is_some(), true);
    }
}
