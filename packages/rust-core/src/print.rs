use super::*;

/// Options for printing the print items.
pub struct PrintOptions {
    /// The width the printer will attempt to keep the line under.
    pub max_width: u32,
    /// The number of columns to count when indenting or using a tab.
    pub indent_width: u8,
    /// Whether to use tabs for indenting.
    pub use_tabs: bool,
    /// The newline character to use when doing a new line.
    pub newline_kind: &'static str,
    // Set this to true and the printer will do additional validation
    // on input strings to ensure the printer is being used correctly.
    // Setting this to true will make things much slower.
    pub is_testing: bool,
}

/// Prints out the print items using the provided
pub fn print<TString, TInfo, TCondition>(
    print_items: Vec<PrintItem<TString, TInfo, TCondition>>,
    options: PrintOptions
) -> String where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    let write_items = get_write_items(print_items, GetWriteItemsOptions {
        indent_width: options.indent_width,
        max_width: options.max_width,
        is_testing: options.is_testing,
    });

    print_write_items(write_items, PrintWriteItemsOptions {
        use_tabs: options.use_tabs,
        newline_kind: options.newline_kind,
        indent_width: options.indent_width,
    })
}
