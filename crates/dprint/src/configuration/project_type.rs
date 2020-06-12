use dprint_core::configuration::ConfigurationDiagnostic;
use crate::utils::get_table_text;

/// Checks if the configuration has a missing "project type" property.
///
/// This is done to encourage companies to support the project. They obviously aren't required to though.
/// Please discuss this with me if you have strong reservations about this. Note that this application took a lot of
/// time, effort, and previous built up knowledge and I'm happy to give it away for free to open source projects,
/// but would like to see companies support it financially even if it's only in a small way.
pub fn handle_project_type_diagnostic(project_type: &Option<String>) -> Option<ConfigurationDiagnostic> {
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
            message: build_message(&project_type_infos, property_name),
        })
    }
}

fn build_message(project_type_infos: &Vec<ProjectTypeInfo>, property_name: &str) -> String {
    let mut message = String::new();
    message.push_str(&format!("The '{}' property is missing in the configuration file.\n\n", property_name));
    message.push_str("You may specify any of the following values and that will suppress this error:\n");
    let option_texts = get_table_text(project_type_infos.iter().map(|info| (info.name, info.description)).collect(), 3);
    for option_text in option_texts {
        message.push_str(&format!("\n * {}", option_text))
    }
    message.push_str("\n\nSee commercial pricing at: https://dprint.dev/pricing");
    message
}

pub struct ProjectTypeInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub fn get_project_type_infos() -> Vec<ProjectTypeInfo> {
    vec![ProjectTypeInfo {
        name: "openSource",
        description: "Dprint is formatting a non-commercial open source project.",
    }, ProjectTypeInfo {
        name: "commercialPaid",
        description: concat!(
            "Dprint is formatting a commercial project and your company paid for a license.\n",
            "Thank you for being part of moving this project forward!"
        ),
    }, ProjectTypeInfo {
        name: "commercialTrial",
        description: "Dprint is formatting a commercial project and you are trying it out for 30 days.",
    }]
}

#[cfg(test)]
mod tests {
    use super::handle_project_type_diagnostic;

    #[test]
    fn it_should_handle_when_project_type_exists() {
        let result = handle_project_type_diagnostic(&Some(String::from("openSource")));
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_be_case_insensitive() {
        // don't get people too upset :)
        let result = handle_project_type_diagnostic(&Some(String::from("opensource")));
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_handle_when_project_type_not_exists() {
        let result = handle_project_type_diagnostic(&None);
        assert_eq!(result.is_some(), true);
        let result = result.unwrap();
        assert_eq!(result.property_name, "projectType");
        assert_eq!(result.message, r#"The 'projectType' property is missing in the configuration file.

You may specify any of the following values and that will suppress this error:

 * openSource      Dprint is formatting a non-commercial open source project.
 * commercialPaid  Dprint is formatting a commercial project and your company paid for a license.
                   Thank you for being part of moving this project forward!
 * commercialTrial Dprint is formatting a commercial project and you are trying it out for 30 days.

See commercial pricing at: https://dprint.dev/pricing"#);
    }

    #[test]
    fn it_should_handle_when_project_type_not_valid_option() {
        let result = handle_project_type_diagnostic(&Some(String::from("test")));
        assert_eq!(result.is_some(), true);
    }
}
