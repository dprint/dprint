use super::*;
use dprint_core::*;
use swc_common::{BytePos, comments::{Comment}};

/// Formats a file.
///
/// # Example
/// ```
/// use dprint_plugin_typescript::*;
///
/// // build the configuration once...
/// let config = ConfigurationBuilder::new()
///     .line_width(80)
///     .force_multi_line_parameters(true)
///     .force_multi_line_arguments(true)
///     .single_quotes(true)
///     .next_control_flow_position(NextControlFlowPosition::SameLine)
///     .build();
///
/// // now format many files (consider parallelizing this)
/// let files_to_format = vec![("path/to/file.ts", "const  t  =  5 ;")];
/// for (file_path, file_text) in files_to_format {
///     let formatted_text = format_text(file_path, file_text, &config);
///     // ...save formatted_text here...
/// }
/// ```
pub fn format_text(file_path: &str, file_text: &str, config: &Configuration) -> Result<Option<String>, String> {
    return swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
        let mut parsed_source_file = parse_to_swc_ast(&file_path, &file_text)?;
        if !should_format_file(&mut parsed_source_file) {
            return Ok(None);
        }

        let print_items = parse(parsed_source_file, config.clone());

        Ok(Some(print(print_items, PrintOptions {
            indent_width: config.indent_width,
            max_width: config.line_width,
            use_tabs: config.use_tabs,
            newline_kind: get_new_line_kind(file_text, config),
        })))
    });

    fn get_new_line_kind(file_text: &str, config: &Configuration) -> &'static str {
        match config.new_line_kind {
            NewLineKind::Unix => "\n",
            NewLineKind::Windows => "\r\n",
            NewLineKind::Auto => {
                let mut found_slash_n = false;
                for c in file_text.as_bytes().iter().rev() {
                    if found_slash_n {
                        if c == &('\r' as u8) {
                            return "\r\n";
                        } else {
                            return "\n";
                        }
                    }

                    if c == &('\n' as u8) {
                        found_slash_n = true;
                    }
                }

                return "\n";
            }
        }
    }

    fn should_format_file(file: &mut ParsedSourceFile) -> bool {
        // just the way it is in swc
        return if file.module.body.is_empty() {
            should_format_based_on_comments(file.trailing_comments.get(&BytePos(0)))
        } else {
            should_format_based_on_comments(file.leading_comments.get(&get_search_position(&file)))
        };

        fn should_format_based_on_comments(comments: Option<&Vec<Comment>>) -> bool {
            if let Some(comments) = comments {
                for comment in comments.iter() {
                    if comment.text.contains("dprint-ignore-file") {
                        return false;
                    }
                }
            }

            return true;
        }

        fn get_search_position(file: &ParsedSourceFile) -> BytePos {
            if let Some(first_statement) = file.module.body.get(0) {
                first_statement.lo()
            } else {
                BytePos(0)
            }
        }
    }
}