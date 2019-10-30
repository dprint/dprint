use super::StringContainer;
use super::printer::*;
use super::PrintItem;
use super::WriteItem;

/// Gets write items from the print items.
pub fn get_write_items<T>(print_items: Vec<PrintItem<T>>, options: PrintOptions) -> Vec<WriteItem<T>> where T : StringContainer {
    let printer = Printer::new(print_items, options);
    printer.print()
}
