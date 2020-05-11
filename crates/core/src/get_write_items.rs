use super::printer::*;
use super::PrintItems;
use super::WriteItem;

/// Options for getting the write items.
pub struct GetWriteItemsOptions {
    /// The width the printer will attempt to keep the line under.
    pub max_width: u32,
    /// The number of columns to count when indenting or using a tab.
    pub indent_width: u8,
}

/// Gets write items from the print items.
pub fn get_write_items(print_items: &PrintItems, options: GetWriteItemsOptions) -> impl Iterator<Item = WriteItem> {
    let printer = Printer::new(print_items.first_node.clone(), options);
    printer.print()
}
