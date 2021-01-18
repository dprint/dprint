use bumpalo::Bump;
use std::cell::RefCell;

use super::*;
use super::utils::{with_bump_allocator, with_bump_allocator_mut};

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

/// Function to create the provided print items and print them out as a string.
///
/// Note: It is unsafe to use the print items created within `get_print_items`
/// outside of the closure, since they are created with a thread local allocator
/// that is reset once this function returns.
pub fn format(get_print_items: impl FnOnce() -> PrintItems, options: PrintOptions) -> String {
    increment_formatting_count();
    let print_items = get_print_items();

    let result = with_bump_allocator_mut(|bump| {
        let result = print_with_allocator(bump, &print_items, &options);
        if decrement_formatting_count() {
            bump.reset();
        }
        result
    });
    result
}

/// Prints out the print items using the provided options.
///
/// Note: This should only be used in rare scenarios. In most cases,
/// use only `dprint_core::formatting::format`.
pub fn print(print_items: PrintItems, options: PrintOptions) -> String {
    // This shouldn't be called without calling `format` because it doesn't
    // reset the allocator.
    panic_if_not_formatting();

    with_bump_allocator(|bump| {
        print_with_allocator(bump, &print_items, &options)
    })
}

fn print_with_allocator(bump: &Bump, print_items: &PrintItems, options: &PrintOptions) -> String {
    let write_items = get_write_items(bump, print_items, GetWriteItemsOptions {
        indent_width: options.indent_width,
        max_width: options.max_width,
    });
    print_write_items(write_items, PrintWriteItemsOptions {
        use_tabs: options.use_tabs,
        new_line_text: options.new_line_text,
        indent_width: options.indent_width,
    })
}

thread_local! {
    static FORMATTING_COUNT: RefCell<u32> = RefCell::new(0);
}

fn increment_formatting_count() {
    FORMATTING_COUNT.with(|formatting_count_cell| {
        let mut formatting_count = formatting_count_cell.borrow_mut();
        *formatting_count += 1;
    })
}

fn decrement_formatting_count() -> bool {
    FORMATTING_COUNT.with(|formatting_count_cell| {
        let mut formatting_count = formatting_count_cell.borrow_mut();
        *formatting_count -= 1;
        *formatting_count == 0
    })
}

fn panic_if_not_formatting() {
    FORMATTING_COUNT.with(|formatting_count_cell| {
        if *formatting_count_cell.borrow() == 0 {
            panic!("dprint_core::formatting::print cannot be called except within the provided closure to dprint_core::formatting::format");
        }
    })
}
