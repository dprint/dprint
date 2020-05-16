use swc_common::{GLOBALS, Globals};
use dprint_core::*;
use dprint_core::configuration::{resolve_new_line_kind};
use std::path::PathBuf;
use super::parsing::parse;
use super::swc::parse_swc_ast;
use super::configuration::Configuration;

/// Formatter for formatting JavaScript and TypeScript code.
///
/// # Example
///
/// ```
/// use std::path::PathBuf;
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
/// let formatter = Formatter::new(config);
///
/// // now format many files (possibly parallelize this)
/// let files_to_format = vec![(PathBuf::from("path/to/file.ts"), "const  t  =  5 ;")];
/// for (file_path, file_text) in files_to_format.iter() {
///     let result = formatter.format_text(file_path, file_text);
///     // save result here...
/// }
/// ```
pub struct Formatter {
    globals: Globals,
    config: Configuration,
}

impl Formatter {
    /// Creates a new formatter with the specified configuration.
    pub fn new(config: Configuration) -> Self {
        Formatter {
            globals: Globals::new(),
            config,
        }
    }

    /// Formats a file.
    ///
    /// Returns the file text `Ok(formatted_text) or an error when it failed to parse.
    pub fn format_text(&self, file_path: &PathBuf, file_text: &str) -> Result<String, String> {
        return self.run(|| {
            if has_ignore_comment(file_text, &self.config) {
                return Ok(String::from(file_text));
            }

            let parsed_source_file = parse_swc_ast(&file_path, &file_text)?;
            let print_items = parse(&parsed_source_file, &self.config);

            // println!("{}", print_items.get_as_text());

            Ok(print(print_items, PrintOptions {
                indent_width: self.config.indent_width,
                max_width: self.config.line_width,
                use_tabs: self.config.use_tabs,
                new_line_text: resolve_new_line_kind(file_text, self.config.new_line_kind),
            }))
        });

        fn has_ignore_comment(file_text: &str, config: &Configuration) -> bool {
            let mut iterator = super::utils::CharIterator::new(file_text.chars());
            iterator.skip_whitespace();
            if iterator.move_next() != Some('/') { return false; }
            match iterator.move_next() {
                Some('/') | Some('*') => {},
                _ => return false,
            }
            iterator.skip_whitespace();
            iterator.check_text(&config.ignore_file_comment_text)
        }
    }

    fn run<F, TReturn>(&self, action: F) -> TReturn where F: FnOnce() -> TReturn {
        // this is what swc does internally
        GLOBALS.set(&self.globals, action)
    }
}
