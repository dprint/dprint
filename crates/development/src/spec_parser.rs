use std::collections::HashMap;

#[derive(PartialEq, Debug)]
pub struct Spec {
    pub file_name: String,
    pub message: String,
    pub file_text: String,
    pub expected_text: String,
    pub is_only: bool,
    pub show_tree: bool,
    pub skip: bool,
    pub skip_format_twice: bool,
    pub config: HashMap<String, String>,
}

pub struct ParseSpecOptions {
    /// The default file name for a parsed spec.
    pub default_file_name: &'static str,
}

pub fn parse_specs(file_text: String, options: &ParseSpecOptions) -> Vec<Spec> {
    // this function needs a rewrite
    let file_text = file_text.replace("\r\n", "\n");
    let (file_path, file_text) = parse_file_path(file_text, options);
    let (config, file_text) = parse_config(file_text);
    let lines = file_text.split("\n").collect();
    let spec_starts = get_spec_starts(&file_path, &lines);
    let mut specs = Vec::new();

    for i in 0..spec_starts.len() {
        let start_index = spec_starts[i];
        let end_index = if spec_starts.len() == i + 1 { lines.len() } else { spec_starts[i + 1] };
        let message_line = lines[start_index];
        let spec = parse_single_spec(&file_path, &message_line, &lines[(start_index + 1)..end_index], &config);

        specs.push(spec);
    }

    return specs;

    fn parse_file_path(file_text: String, options: &ParseSpecOptions) -> (String, String) {
        if !file_text.starts_with("--") {
            return (options.default_file_name.into(), file_text);
        }
        let last_index = file_text.find("--\n").expect("Could not find final --");

        (file_text["--".len()..last_index].trim().into(), file_text[(last_index + "--\n".len())..].into())
    }

    fn parse_config(file_text: String) -> (HashMap<String, String>, String) {
        if !file_text.starts_with("~~") {
            return (HashMap::new(), file_text);
        }
        let last_index = file_text.find("~~\n").expect("Could not find final ~~\\n");

        let config_text = file_text["~~".len()..last_index].replace("\n", "");
        let mut config: HashMap<String, String> = HashMap::new();

        for item in config_text.split(",") {
            let first_colon = item.find(":").expect("Could not find colon in config option.");
            let key = item[0..first_colon].trim();
            let value = item[first_colon + ":".len()..].trim();

            config.insert(key.into(), value.into());
        }

        (config, file_text[(last_index + "~~\n".len())..].into())
    }

    fn get_spec_starts(file_name: &str, lines: &Vec<&str>) -> Vec<usize> {
        let mut result = Vec::new();
        let message_separator = get_message_separator(file_name);

        if !lines.first().unwrap().starts_with(message_separator) {
            panic!("All spec files should start with a message. (ex. {0} Message {0})", message_separator);
        }

        for i in 0..lines.len() {
            if lines[i].starts_with(message_separator) {
                result.push(i);
            }
        }

        result
    }

    fn parse_single_spec(file_name: &str, message_line: &str, lines: &[&str], config: &HashMap<String, String>) -> Spec {
        let file_text = lines.join("\n");
        let parts = file_text.split("[expect]").collect::<Vec<&str>>();
        let start_text = parts[0][0..parts[0].len() - "\n".len()].into(); // remove last newline
        let expected_text = parts[1]["\n".len()..].into(); // remove first newline
        let lower_case_message_line = message_line.to_ascii_lowercase();
        let message_separator = get_message_separator(file_name);

        Spec {
            file_name: String::from(file_name),
            message: message_line[message_separator.len()..message_line.len() - message_separator.len()].trim().into(),
            file_text: start_text,
            expected_text,
            is_only: lower_case_message_line.find("(only)").is_some(),
            skip: lower_case_message_line.find("(skip)").is_some(),
            skip_format_twice: lower_case_message_line.find("(skip-format-twice)").is_some(),
            show_tree: lower_case_message_line.find("(tree)").is_some(),
            config: config.clone(),
        }
    }

    fn get_message_separator(file_name: &str) -> &'static str {
        return if file_name.ends_with(".md") {
            "!!"
        } else {
            "=="
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses() {
        let specs = parse_specs(vec![
            "== message 1 ==",
            "start",
            "multiple",
            "",
            "[expect]",
            "expected",
            "multiple",
            "",
            "== message 2 (only) (tree) (skip) (skip-format-twice) ==",
            "start2",
            "",
            "[expect]",
            "expected2",
            "",
        ].join("\n"), &ParseSpecOptions { default_file_name: "test.ts" });

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0], Spec {
            file_name: "test.ts".into(),
            file_text: "start\nmultiple\n".into(),
            expected_text: "expected\nmultiple\n".into(),
            message: "message 1".into(),
            is_only: false,
            show_tree: false,
            skip: false,
            skip_format_twice: false,
            config: HashMap::new(),
        });
        assert_eq!(specs[1], Spec {
            file_name: "test.ts".into(),
            file_text: "start2\n".into(),
            expected_text: "expected2\n".into(),
            message: "message 2 (only) (tree) (skip) (skip-format-twice)".into(),
            is_only: true,
            show_tree: true,
            skip: true,
            skip_format_twice: true,
            config: HashMap::new(),
        });
    }

    #[test]
    fn it_parses_with_file_name() {
        let specs = parse_specs(vec![
            "-- asdf.ts --",
            "== message ==",
            "start",
            "[expect]",
            "expected",
        ].join("\n"), &ParseSpecOptions { default_file_name: "test.ts" });

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0], Spec {
            file_name: "asdf.ts".into(),
            file_text: "start".into(),
            expected_text: "expected".into(),
            message: "message".into(),
            is_only: false,
            show_tree: false,
            skip: false,
            skip_format_twice: false,
            config: HashMap::new(),
        });
    }

    #[test]
    fn it_parses_with_config() {
        let specs = parse_specs(vec![
            "-- asdf.ts --",
            "~~ test.test: other, lineWidth: 40 ~~",
            "== message ==",
            "start",
            "[expect]",
            "expected",
        ].join("\n"), &ParseSpecOptions { default_file_name: "test.ts" });

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0], Spec {
            file_name: "asdf.ts".into(),
            file_text: "start".into(),
            expected_text: "expected".into(),
            message: "message".into(),
            is_only: false,
            show_tree: false,
            skip: false,
            skip_format_twice: false,
            config: [("test.test".into(), "other".into()), ("lineWidth".into(), "40".into())].iter().cloned().collect(),
        });
    }

    #[test]
    fn it_parses_markdown() {
        let specs = parse_specs(vec![
            "!! message 1 !!",
            "start",
            "multiple",
            "",
            "[expect]",
            "expected",
            "multiple",
            "",
            "!! message 2 (only) (tree) (skip) (skip-format-twice) !!",
            "start2",
            "",
            "[expect]",
            "expected2",
            "",
        ].join("\n"), &ParseSpecOptions { default_file_name: "test.md" });

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0], Spec {
            file_name: "test.md".into(),
            file_text: "start\nmultiple\n".into(),
            expected_text: "expected\nmultiple\n".into(),
            message: "message 1".into(),
            is_only: false,
            show_tree: false,
            skip: false,
            skip_format_twice: false,
            config: HashMap::new(),
        });
        assert_eq!(specs[1], Spec {
            file_name: "test.md".into(),
            file_text: "start2\n".into(),
            expected_text: "expected2\n".into(),
            message: "message 2 (only) (tree) (skip) (skip-format-twice)".into(),
            is_only: true,
            show_tree: true,
            skip: true,
            skip_format_twice: true,
            config: HashMap::new(),
        });
    }
}
