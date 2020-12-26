use super::*;
use bumpalo::Bump;

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

thread_local! {
    static LOCAL_BUMP_ALLOCATOR: std::cell::RefCell<Bump> = std::cell::RefCell::new(Bump::new());
}

/// Prints out the print items using the provided
pub fn print(print_items: PrintItems, options: PrintOptions) -> String {
    LOCAL_BUMP_ALLOCATOR.with(|bump_cell| {
        let mut bump_borrow = bump_cell.borrow_mut();
        let write_items = get_write_items(&bump_borrow, &print_items, GetWriteItemsOptions {
            indent_width: options.indent_width,
            max_width: options.max_width,
        });

        let result = print_write_items(write_items, PrintWriteItemsOptions {
            use_tabs: options.use_tabs,
            new_line_text: options.new_line_text,
            indent_width: options.indent_width,
        });

        bump_borrow.reset();
        result
    })
}
