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
    pub new_line_text: &'static str,
}

/// Prints out the print items using the provided
pub fn print<TString, TInfo, TCondition>(
    print_items: PrintItems<TString, TInfo, TCondition>,
    options: PrintOptions
) -> String where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    let write_items = get_write_items(&print_items, GetWriteItemsOptions {
        indent_width: options.indent_width,
        max_width: options.max_width,
    });

    print_write_items(write_items, PrintWriteItemsOptions {
        use_tabs: options.use_tabs,
        new_line_text: options.new_line_text,
        indent_width: options.indent_width,
    })
}
