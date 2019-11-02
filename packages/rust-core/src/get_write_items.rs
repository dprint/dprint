use super::StringRef;
use super::InfoRef;
use super::ConditionRef;
use super::printer::*;
use super::PrintItem;
use super::WriteItem;

/// Gets write items from the print items.
pub fn get_write_items<TString, TInfo, TCondition>(
    print_items: Vec<PrintItem<TString, TInfo, TCondition>>,
    options: PrintOptions
) -> Vec<WriteItem<TString>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    let printer = Printer::new(print_items, options);
    printer.print()
}
