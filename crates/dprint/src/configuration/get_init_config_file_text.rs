use dprint_core::plugins::PLUGIN_SYSTEM_SCHEMA_VERSION;

use crate::environment::Environment;
use crate::plugins::read_info_file;
use crate::types::ErrBox;
use crate::utils::get_table_text;

use super::get_project_type_infos;

pub async fn get_init_config_file_text(environment: &impl Environment) -> Result<String, ErrBox> {
    let project_type_name = get_project_type_name(environment)?;

    let info = match read_info_file(environment).await {
        Ok(info) => {
            if info.plugin_system_schema_version != PLUGIN_SYSTEM_SCHEMA_VERSION {
                environment.log_error(&format!(
                    concat!(
                        "You are using an old version of dprint so the created config file may not be as helpful of a starting point. ",
                        "Consider upgrading to support new plugins. ",
                        "Plugin system schema version is {}, latest is {}."
                    ),
                    PLUGIN_SYSTEM_SCHEMA_VERSION,
                    info.plugin_system_schema_version,
                ));
                None
            } else {
                Some(info)
            }
        },
        Err(err) => {
            environment.log_error(&format!(
                concat!(
                    "There was a problem getting the latest plugin info. ",
                    "The created config file may not be as helpful of a starting point. ",
                    "Error: {}"
                ),
                err.to_string()
            ));
            None
        }
    };

    let mut json_text = String::from("{\n");
    json_text.push_str("  \"$schema\": \"https://dprint.dev/schemas/v0.json\",\n");
    json_text.push_str(&format!("  \"projectType\": \"{}\",\n", project_type_name));

    if let Some(info) = &info {
        for plugin in info.latest_plugins.iter() {
            // Put the brace on the next line so the user doesn't have to as soon as they
            // go to add options.
            json_text.push_str(&format!("  \"{}\": {{\n", plugin.config_key));
            if !plugin.config_schema_url.is_empty() {
                json_text.push_str(&format!("    \"$schema\": \"{}\"\n", plugin.config_schema_url));
            }
            json_text.push_str("  },\n");
        }

        json_text.push_str("  \"includes\": [\"**/*.{");
        json_text.push_str(&info.latest_plugins.iter().flat_map(|p| p.file_extensions.iter()).map(|x| x.to_owned()).collect::<Vec<_>>().join(","));
        json_text.push_str("}\"],\n")
    } else {
        json_text.push_str("  \"includes\": [\"**/*.{ts,tsx,js,jsx,json}\"],\n");
    }

    json_text.push_str("  \"excludes\": [],\n");
    json_text.push_str("  \"plugins\": [\n");

    if let Some(info) = &info {
        let plugin_count = info.latest_plugins.len();
        for (i, plugin) in info.latest_plugins.iter().enumerate() {
            json_text.push_str(&format!("    \"{}\"", plugin.url));

            if i < plugin_count - 1 { json_text.push_str(","); }
            json_text.push_str("\n");
        }
    } else {
        json_text.push_str("    // specify plugin urls here\n");
    }

    json_text.push_str("  ]\n}\n");

    Ok(json_text)
}

fn get_project_type_name(environment: &impl Environment) -> Result<&'static str, ErrBox> {
    let project_type_infos = get_project_type_infos();
    environment.log("What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n");
    let options = get_table_text(project_type_infos.iter().map(|info| (info.name, info.description)).collect(), 2);
    let project_type_index = environment.get_selection(&options)?;
    Ok(project_type_infos[project_type_index].name)
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use crate::environment::TestEnvironment;
    use crate::plugins::REMOTE_INFO_URL;
    use super::*;

    #[tokio::test]
    async fn should_get_initialization_text_when_can_access_url() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 1,
    "pluginSystemSchemaVersion": 1,
    "latest": [{
        "name": "dprint-plugin-typescript",
        "version": "0.17.2",
        "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
        "configKey": "typescript",
        "fileExtensions": ["ts", "tsx"]
    }, {
        "name": "dprint-plugin-jsonc",
        "version": "0.2.3",
        "url": "https://plugins.dprint.dev/json-0.2.3.wasm",
        "configKey": "json",
        "fileExtensions": ["json"],
        "configSchemaUrl": "https://plugins.dprint.dev/schemas/json-v1.json"
    }]
}"#.as_bytes());
        let text = get_init_config_file_text(&environment).await.unwrap();
        assert_eq!(
            text,
            r#"{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "openSource",
  "typescript": {
  },
  "json": {
    "$schema": "https://plugins.dprint.dev/schemas/json-v1.json"
  },
  "includes": ["**/*.{ts,tsx,json}"],
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm",
    "https://plugins.dprint.dev/json-0.2.3.wasm"
  ]
}
"#
        );

        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.get_logged_messages(), get_standard_logged_messages());
    }

    #[tokio::test]
    async fn should_get_initialization_text_when_cannot_access_url() {
        let environment = TestEnvironment::new();
        let text = get_init_config_file_text(&environment).await.unwrap();
        assert_eq!(
            text,
            r#"{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "openSource",
  "includes": ["**/*.{ts,tsx,js,jsx,json}"],
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
        );
        assert_eq!(environment.get_logged_errors(), vec![
            concat!(
                "There was a problem getting the latest plugin info. ",
                "The created config file may not be as helpful of a starting point. ",
                "Error: Could not find file at url https://plugins.dprint.dev/info.json"
            )
        ]);
        assert_eq!(environment.get_logged_messages(), get_standard_logged_messages());
    }

    #[tokio::test]
    async fn should_get_initialization_text_when_selecting_other_option() {
        let environment = TestEnvironment::new();
        environment.set_selection_result(1);
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 1,
    "pluginSystemSchemaVersion": 1,
    "latest": [{
        "name": "dprint-plugin-typescript",
        "version": "0.17.2",
        "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
        "configKey": "typescript",
        "fileExtensions": ["ts"]
    }]
}"#.as_bytes());
        let text = get_init_config_file_text(&environment).await.unwrap();
        assert_eq!(
            text,
            r#"{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "commercialPaid",
  "typescript": {
  },
  "includes": ["**/*.{ts}"],
  "excludes": [],
  "plugins": [
    "https://plugins.dprint.dev/typescript-0.17.2.wasm"
  ]
}
"#
        );

        assert_eq!(environment.get_logged_errors().len(), 0);
        assert_eq!(environment.get_logged_messages(), get_standard_logged_messages());
    }

    #[tokio::test]
    async fn should_get_initialization_text_when_old_plugin_system() {
        let environment = TestEnvironment::new();
        environment.add_remote_file(REMOTE_INFO_URL, r#"{
    "schemaVersion": 1,
    "pluginSystemSchemaVersion": 2, // this is 2 instead of 1
    "latest": [{
        "name": "dprint-plugin-typescript",
        "version": "0.17.2",
        "url": "https://plugins.dprint.dev/typescript-0.17.2.wasm",
        "configKey": "typescript",
        "fileExtensions": ["ts"]
    }]
}"#.as_bytes());
        let text = get_init_config_file_text(&environment).await.unwrap();
        assert_eq!(
            text,
            r#"{
  "$schema": "https://dprint.dev/schemas/v0.json",
  "projectType": "openSource",
  "includes": ["**/*.{ts,tsx,js,jsx,json}"],
  "excludes": [],
  "plugins": [
    // specify plugin urls here
  ]
}
"#
        );
        assert_eq!(environment.get_logged_errors(), vec![
            concat!(
                "You are using an old version of dprint so the created config file may not be as helpful of a starting point. ",
                "Consider upgrading to support new plugins. ",
                "Plugin system schema version is 1, latest is 2."
            ),
        ]);
        assert_eq!(environment.get_logged_messages(), get_standard_logged_messages());
    }

    fn get_standard_logged_messages() -> Vec<&'static str> {
        vec!["What kind of project will dprint be formatting?\n\nSee commercial pricing at: https://dprint.dev/pricing\n"]
    }
}
