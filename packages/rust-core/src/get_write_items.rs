use super::StringTrait;
use super::InfoTrait;
use super::ConditionTrait;
use super::printer::*;
use super::PrintItems;
use super::WriteItem;

/// Options for getting the write items.
pub struct GetWriteItemsOptions {
    /// The width the printer will attempt to keep the line under.
    pub max_width: u32,
    /// The number of columns to count when indenting or using a tab.
    pub indent_width: u8,
    // Set this to true and the printer will do additional validation
    // on input strings to ensure the printer is being used correctly.
    // Setting this to true will make things much slower.
    pub is_testing: bool,
}

/// Gets write items from the print items.
pub fn get_write_items<TString, TInfo, TCondition>(
    print_items: &PrintItems<TString, TInfo, TCondition>,
    options: GetWriteItemsOptions
) -> impl Iterator<Item = WriteItem<TString>> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    let printer = Printer::new(print_items.first_node.clone(), options);
    printer.print()
}
