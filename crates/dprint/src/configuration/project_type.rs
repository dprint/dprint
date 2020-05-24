use dprint_core::configuration::ConfigurationDiagnostic;
use crate::utils::get_table_text;
use super::{ConfigMapValue, ConfigMap};

/// Checks if the configuration has a missing "projectType" property.
///
/// This is done to encourage companies to support the project. They obviously aren't required to though.
/// Please discuss this with me if you have strong reservations about this. Note that this library took a lot of
/// time, effort, and previous built up knowledge and I'm happy to give it away for free to open source projects,
/// but would like to see companies support it financially even if it's only in a small way.
pub fn handle_project_type_diagnostic(config: &mut ConfigMap) -> Option<ConfigurationDiagnostic> {
    let project_type_infos = get_project_type_infos();
    let property_name = "projectType";
    let has_project_type_config = match config.get(property_name) {
        Some(ConfigMapValue::String(project_type)) => project_type_infos.iter().any(|info| info.name.to_lowercase() == project_type.to_lowercase()),
        _ => false,
    };

    config.remove(property_name);

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
    message.push_str("You may specify any of the following values and that will suppress this error.\n");
    let option_texts = get_table_text(project_type_infos.iter().map(|info| (info.name, info.description)).collect(), 3);
    for option_text in option_texts {
        message.push_str(&format!("\n * {}", option_text))
    }
    message.push_str("\n\nSponsor at: https://dprint.dev/sponsor");
    message
}

pub struct ProjectTypeInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub fn get_project_type_infos() -> Vec<ProjectTypeInfo> {
    vec![ProjectTypeInfo {
        name: "openSource",
        description: "Dprint is formatting an open source project.",
    }, ProjectTypeInfo {
        name: "commercialSponsored",
        description: concat!(
            "Dprint is formatting a commercial project and your company sponsored dprint.\n",
            "Thank you for being part of moving this project forward!"
        ),
    }, ProjectTypeInfo {
        name: "commercialDidNotSponsor",
        description: concat!(
            "Dprint is formatting a commercial project and you are just trying it out or don't want to sponsor.\n",
            "If you are in the financial position to do so, please take the time to sponsor.\n"
        ),
    }]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::handle_project_type_diagnostic;
    use super::super::ConfigMapValue;

    #[test]
    fn it_should_handle_when_project_type_exists() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), ConfigMapValue::String(String::from("openSource")));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_be_case_insensitive() {
        // don't get people too upset :)
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), ConfigMapValue::String(String::from("opensource")));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_handle_when_project_type_not_exists() {
        let mut config = HashMap::new();
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(result.is_some(), true);
        let result = result.unwrap();
        assert_eq!(result.property_name, "projectType");
        assert_eq!(result.message, r#"The 'projectType' property is missing in the configuration file.

You may specify any of the following values and that will suppress this error.

 * openSource              Dprint is formatting an open source project.
 * commercialSponsored     Dprint is formatting a commercial project and your company sponsored dprint.
                           Thank you for being part of moving this project forward!
 * commercialDidNotSponsor Dprint is formatting a commercial project and you are just trying it out or don't want to sponsor.
                           If you are in the financial position to do so, please take the time to sponsor.

Sponsor at: https://dprint.dev/sponsor"#);
    }

    #[test]
    fn it_should_handle_when_project_type_not_string() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), ConfigMapValue::HashMap(HashMap::new()));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove regardless
        assert_eq!(result.is_some(), true);
    }

    #[test]
    fn it_should_handle_when_project_type_not_valid_option() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), ConfigMapValue::String(String::from("test")));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove regardless
        assert_eq!(result.is_some(), true);
    }
}
