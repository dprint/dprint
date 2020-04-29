use std::collections::HashMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use super::StringOrHashMap;

/// Checks if the configuration has a missing "projectType" property.
///
/// This is done to encourage companies to support the project. They obviously aren't required to though.
/// Please discuss this with me if you have strong reservations about this. Note that this library took a lot of
/// time, effort, and previous built up knowledge and I'm happy to give it away for free to open source projects,
/// but would like to see companies support it financially even if it's only in a small way.
pub fn handle_project_type_diagnostic(config: &mut HashMap<String, StringOrHashMap>) -> Option<ConfigurationDiagnostic> {
    let project_type_infos = get_project_type_infos();
    let property_name = "projectType";
    let has_project_type_config = match config.get(property_name) {
        Some(StringOrHashMap::String(project_type)) => project_type_infos.iter().any(|(k, _)| k.to_lowercase() == project_type.to_lowercase()),
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

fn build_message(project_type_infos: &Vec<(&'static str, &'static str)>, property_name: &str) -> String {
    let largest_name_len = {
        let mut key_lens = project_type_infos.iter().map(|(k, _)| k.len()).collect::<Vec<_>>();
        key_lens.sort();
        key_lens.pop().unwrap_or(0)
    };
    let mut message = String::new();
    message.push_str(&format!("The '{}' property is missing in the configuration file. ", property_name));
    message.push_str("You may specify any of the following possible values according to your conscience and that will suppress this warning.\n");
    for project_type_info in project_type_infos {
        message.push_str(&format!("\n * {}", project_type_info.0));
        message.push_str(&" ".repeat(largest_name_len - project_type_info.0.len() + 1));
        message.push_str(project_type_info.1);
    }
    message
}

fn get_project_type_infos() -> Vec<(&'static str, &'static str)> {
    vec![(
        "openSource",
        "Dprint is formatting an open source project."
    ), (
        "commercialSponsored",
        "Dprint is formatting a commercial project and your company sponsored dprint."
    ), (
        "commercialDidNotSponsor",
        "Dprint is formatting a commercial project and you want to forever enshrine your name in source control for having specified this."
    )]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::handle_project_type_diagnostic;
    use super::super::StringOrHashMap;

    #[test]
    fn it_should_handle_when_project_type_exists() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), StringOrHashMap::String(String::from("openSource")));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove
        assert_eq!(result.is_none(), true);
    }

    #[test]
    fn it_should_be_case_insensitive() {
        // don't get people too upset :)
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), StringOrHashMap::String(String::from("opensource")));
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
        assert_eq!(result.message, r#"The 'projectType' property is missing in the configuration file. You may specify any of the following possible values according to your conscience and that will suppress this warning.

 * openSource              Dprint is formatting an open source project.
 * commercialSponsored     Dprint is formatting a commercial project and your company sponsored dprint.
 * commercialDidNotSponsor Dprint is formatting a commercial project and you want to forever enshrine your name in source control for having specified this."#);
    }

    #[test]
    fn it_should_handle_when_project_type_not_string() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), StringOrHashMap::HashMap(HashMap::new()));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove regardless
        assert_eq!(result.is_some(), true);
    }

    #[test]
    fn it_should_handle_when_project_type_not_valid_option() {
        let mut config = HashMap::new();
        config.insert(String::from("projectType"), StringOrHashMap::String(String::from("test")));
        let result = handle_project_type_diagnostic(&mut config);
        assert_eq!(config.len(), 0); // should remove regardless
        assert_eq!(result.is_some(), true);
    }
}
