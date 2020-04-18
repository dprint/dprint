use swc_common::{GLOBALS, Globals, BytePos, comments::{Comment}};
use dprint_core::*;
use dprint_core::configuration::{resolve_new_line_kind};
use super::*;
use super::configuration::Configuration;

/// Formatter for formatting JavaScript and TypeScript code.
///
/// # Example
///
/// ```
/// use dprint_plugin_typescript::*;
/// use dprint_plugin_typescript::configuration::*;
///
/// // build the configuration once...
/// let config = ConfigurationBuilder::new()
///     .line_width(80)
///     .prefer_hanging(true)
///     .prefer_single_line(false)
///     .quote_style(QuoteStyle::PreferSingle)
///     .next_control_flow_position(NextControlFlowPosition::SameLine)
///     .build();
///
/// // create the formatter
/// let formatter = Formatter::new(&config);
///
/// // now format many files (possibly parallelize this)
/// let files_to_format = vec![("path/to/file.ts", "const  t  =  5 ;")];
/// for (file_path, file_text) in files_to_format {
///     let formatted_text = formatter.format_text(file_path, file_text);
///     // ...save formatted_text here...
/// }
/// ```
pub struct Formatter<'a> {
    globals: Globals,
    config: &'a Configuration,
}

impl<'a> Formatter<'a> {
    /// Creates a new formatter from the specified configuration.
    pub fn new(config: &'a Configuration) -> Self {
        Formatter {
            globals: Globals::new(),
            config,
        }
    }

    /// Formats a file.
    ///
    /// Returns the file text when the file was formatted, `None` when the file had an ignore comment, and
    /// an error when it failed to parse.
    pub fn format_text(&self, file_path: &str, file_text: &str) -> Result<Option<String>, String> {
        return self.run(|| {
            let mut parsed_source_file = parse_swc_ast(&file_path, &file_text)?;
            if !should_format_file(&mut parsed_source_file) {
                return Ok(None);
            }

            let print_items = parse(parsed_source_file, &self.config);

            // println!("{}", print_items.get_as_text());

            Ok(Some(print(print_items, PrintOptions {
                indent_width: self.config.indent_width,
                max_width: self.config.line_width,
                use_tabs: self.config.use_tabs,
                new_line_text: resolve_new_line_kind(file_text, self.config.new_line_kind),
            })))
        });

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

    fn run<F, TReturn>(&self, action: F) -> TReturn where F: FnOnce() -> TReturn {
        // this is what swc does internally
        GLOBALS.set(&self.globals, action)
    }
}
