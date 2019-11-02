use super::WriteItem;
use super::print_items::*;
use super::writer::*;
use std::collections::HashMap;
use std::mem;
use std::rc::Rc;

/// Options for printing the print items.
pub struct PrintOptions {
    /// The width the printer will attempt to keep the line under.
    pub max_width: u32,
    /// The number of columns to count when indenting or using a tab.
    pub indent_width: u8,
    // Set this to true and the printer will do additional validation
    // on input strings to ensure the printer is being used correctly.
    // Setting this to true will make things much slower.
    pub is_testing: bool,
}

struct SavePoint<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    // Unique id
    pub id: u32,
    /// Name for debugging purposes.
    pub name: String,
    pub new_line_group_depth: u16,
    pub writer_state: WriterState<TString>,
    pub possible_new_line_save_point: Box<Option<SavePoint<TString, TInfo, TCondition>>>,
    pub container: PrintItemContainer<TString, TInfo, TCondition>,
    pub current_indexes: Vec<isize>,
}

impl<TString, TInfo, TCondition> Clone for SavePoint<TString, TInfo, TCondition> where TString : StringRef, TInfo: InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn clone(&self) -> SavePoint<TString, TInfo, TCondition> {
        SavePoint {
            id: self.id,
            name: self.name.clone(),
            new_line_group_depth: self.new_line_group_depth,
            writer_state: self.writer_state.clone(),
            possible_new_line_save_point: Box::new((*self.possible_new_line_save_point).as_ref().map(|x| x.clone())),
            container: self.container.clone(),
            current_indexes: self.current_indexes.clone(),
        }
    }
}

struct PrintItemContainer<TString, TInfo, TCondition> where TString : StringRef, TInfo: InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    parent: Box<Option<PrintItemContainer<TString, TInfo, TCondition>>>,
    items: Rc<Vec<PrintItem<TString, TInfo, TCondition>>>,
}

impl<TString, TInfo, TCondition> Clone for PrintItemContainer<TString, TInfo, TCondition> where TString : StringRef, TInfo: InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn clone(&self) -> PrintItemContainer<TString, TInfo, TCondition> {
        PrintItemContainer {
            parent: Box::new((*self.parent).as_ref().map(|x| x.clone())),
            items: self.items.clone(),
        }
    }
}

pub struct Printer<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    possible_new_line_save_point: Option<SavePoint<TString, TInfo, TCondition>>,
    new_line_group_depth: u16,
    container: PrintItemContainer<TString, TInfo, TCondition>,
    current_indexes: Vec<isize>, // todo: usize?
    save_point_increment: u32,
    writer: Writer<TString>,
    resolved_conditions: HashMap<usize, bool>,
    resolved_infos: HashMap<usize, WriterInfo>,
    look_ahead_condition_save_points: HashMap<usize, SavePoint<TString, TInfo, TCondition>>,
    look_ahead_info_save_points: HashMap<usize, SavePoint<TString, TInfo, TCondition>>,
    max_width: u32,
    is_exiting_condition: bool,
    is_testing: bool,
}

impl<TString, TInfo, TCondition> Printer<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub fn new(items: Vec<PrintItem<TString, TInfo, TCondition>>, options: PrintOptions) -> Printer<TString, TInfo, TCondition> {
        Printer {
            possible_new_line_save_point: Option::None,
            new_line_group_depth: 0,
            container: PrintItemContainer {
                parent: Box::new(Option::None),
                items: Rc::new(items),
            },
            current_indexes: vec![0 as isize],
            save_point_increment: 0,
            writer: Writer::new(WriterOptions {
                indent_width: options.indent_width,
            }),
            resolved_conditions: HashMap::new(),
            resolved_infos: HashMap::new(),
            look_ahead_condition_save_points: HashMap::new(),
            look_ahead_info_save_points: HashMap::new(),
            max_width: options.max_width,
            is_exiting_condition: false,
            is_testing: options.is_testing,
        }
    }

    /// Turns the print items into a collection of writer items according to the options.
    pub fn print(mut self) -> Vec<WriteItem<TString>> { // drop self
        loop {
            while self.current_indexes[self.current_indexes.len() - 1] < self.container.items.len() as isize {
                let index = self.current_indexes[self.current_indexes.len() - 1];
                let print_item = self.container.items.get(index as usize).unwrap().clone();
                self.handle_print_item(&print_item);
                let last_index = self.current_indexes.len() - 1;
                self.current_indexes[last_index] += 1;
            }

            let parent_container = self.container.parent;
            if parent_container.is_none() {
                // no parent, we're done
                break;
            }

            self.container = parent_container.unwrap();
            self.current_indexes.pop();
            let last_index = self.current_indexes.len() - 1;
            self.current_indexes[last_index] += 1;

            // self.log_writer_for_debugging();
        }

        self.writer.get_items()
    }

    pub fn get_writer_info(&self) -> WriterInfo {
        WriterInfo {
            line_start_indent_level: self.writer.get_line_start_indent_level(),
            line_start_column_number: self.writer.get_line_start_column_number(),
            line_number: self.writer.get_line_number(),
            column_number: self.writer.get_line_column(),
            indent_level: self.writer.get_indentation_level(),
        }
    }

    pub fn get_resolved_info(&mut self, info: &TInfo) -> Option<WriterInfo> {
        let resolved_info = self.resolved_infos.get(&info.get_unique_id()).map(|x| x.to_owned());
        if resolved_info.is_none() && !self.look_ahead_info_save_points.contains_key(&info.get_unique_id()) {
            let save_point = self.create_save_point_for_restoring_condition(if self.is_testing { &info.get_name() } else { "" });
            self.look_ahead_info_save_points.insert(info.get_unique_id(), save_point);
        }

        resolved_info.map(|x| x.to_owned())
    }

    pub fn get_resolved_condition(&mut self, condition: &TCondition) -> Option<bool> {
        let optional_result = self.resolved_conditions.get(&condition.get_unique_id()).map(|x| x.to_owned());

        if optional_result.is_none() {
            if !self.look_ahead_condition_save_points.contains_key(&condition.get_unique_id()) {
                let save_point = self.create_save_point_for_restoring_condition(if self.is_testing { &condition.get_name() } else { "" });
                self.look_ahead_condition_save_points.insert(condition.get_unique_id(), save_point);
            }
        } else {
            self.restore_to_condition_save_point_if_necessary(&condition);
        }

        optional_result.map(|x| x.to_owned())
    }

    fn handle_print_item(&mut self, print_item: &PrintItem<TString, TInfo, TCondition>) {
        match print_item {
            PrintItem::String(text) => self.handle_string(text),
            PrintItem::Condition(condition) => self.handle_condition(condition),
            PrintItem::Info(info) => self.handle_info(info),
            // signals
            PrintItem::NewLine => self.write_new_line(),
            PrintItem::Tab => self.writer.tab(),
            PrintItem::ExpectNewLine => {
                self.writer.mark_expect_new_line();
                self.possible_new_line_save_point = Option::None;
            }
            PrintItem::PossibleNewLine => self.mark_possible_new_line_if_able(),
            PrintItem::SpaceOrNewLine => {
                if self.is_above_max_width(1) {
                    let optional_save_state = mem::replace(&mut self.possible_new_line_save_point, Option::None);
                    if optional_save_state.is_none() {
                        self.write_new_line();
                    } else if let Some(save_state) = optional_save_state {
                        if save_state.new_line_group_depth >= self.new_line_group_depth {
                            self.write_new_line();
                        } else {
                            self.update_state_to_save_point(save_state, true);
                        }
                    }
                } else {
                    self.mark_possible_new_line_if_able();
                    self.writer.space();
                }
            }
            PrintItem::StartIndent => self.writer.start_indent(),
            PrintItem::FinishIndent => self.writer.finish_indent(),
            PrintItem::StartNewLineGroup => self.new_line_group_depth += 1,
            PrintItem::FinishNewLineGroup => self.new_line_group_depth -= 1,
            PrintItem::SingleIndent => self.writer.single_indent(),
            PrintItem::StartIgnoringIndent => self.writer.start_ignoring_indent(),
            PrintItem::FinishIgnoringIndent => self.writer.finish_ignoring_indent(),
        }
    }

    fn write_new_line(&mut self) {
        self.writer.new_line();
        self.possible_new_line_save_point = Option::None;
    }

    fn create_save_point(&mut self, name: &str) -> SavePoint<TString, TInfo, TCondition> {
        self.save_point_increment += 1;
        SavePoint {
            id: self.save_point_increment,
            name: String::from(name),
            possible_new_line_save_point: Box::new(self.possible_new_line_save_point.clone()),
            new_line_group_depth: self.new_line_group_depth,
            current_indexes: self.current_indexes.clone(),
            container: self.container.clone(),
            writer_state: self.writer.get_state(),
        }
    }

    fn create_save_point_for_restoring_condition(&mut self, name: &str) -> SavePoint<TString, TInfo, TCondition> {
        let mut save_point = self.create_save_point(name);
        let last_index = save_point.current_indexes.len() - 1;
        save_point.current_indexes[last_index] -= 1;
        save_point
    }

    fn mark_possible_new_line_if_able(&mut self) {
        if let Some(new_line_save_point) = &self.possible_new_line_save_point {
            if self.new_line_group_depth > new_line_save_point.new_line_group_depth {
                return;
            }
        }

        self.possible_new_line_save_point = Some(self.create_save_point("newline"));
    }

    fn is_above_max_width(&self, offset: u32) -> bool {
        self.writer.get_line_column() + 1 + offset > self.max_width
    }

    fn update_state_to_save_point(&mut self, save_point: SavePoint<TString, TInfo, TCondition>, is_for_new_line: bool) {
        self.writer.set_state(save_point.writer_state);
        self.possible_new_line_save_point = if is_for_new_line { Option::None } else { *save_point.possible_new_line_save_point };
        self.container = save_point.container;
        self.current_indexes = save_point.current_indexes;
        self.new_line_group_depth = save_point.new_line_group_depth;

        if is_for_new_line {
            self.write_new_line();
        }
    }

    fn handle_info(&mut self, info: &TInfo) {
        self.resolved_infos.insert(info.get_unique_id(), self.get_writer_info());
        let option_save_point = self.look_ahead_info_save_points.remove(&info.get_unique_id());
        if let Some(save_point) = option_save_point {
            self.update_state_to_save_point(save_point, false);
        }
    }

    fn handle_condition(&mut self, condition: &TCondition) {
        let condition_value = self.get_condition_value(&condition);

        if self.is_exiting_condition {
            self.is_exiting_condition = false;
            return;
        }

        if condition_value.is_some() && condition_value.unwrap() {
            if let Some(true_path) = &condition.get_true_path() {
                let new_parent = mem::replace(&mut self.container, PrintItemContainer { parent: Box::new(Option::None), items: Rc::new(Vec::new()) });
                self.container = PrintItemContainer {
                    items: true_path.clone(),
                    parent: Box::new(Some(new_parent)),
                };
                self.current_indexes.push(-1);
            }
        } else {
            if let Some(false_path) = &condition.get_false_path() {
                let new_parent = mem::replace(&mut self.container, PrintItemContainer { parent: Box::new(Option::None), items: Rc::new(Vec::new()) });
                self.container = PrintItemContainer {
                    items: false_path.clone(),
                    parent: Box::new(Some(new_parent)),
                };
                self.current_indexes.push(-1);
            }
        }
    }

    fn get_condition_value(&mut self, condition: &TCondition) -> Option<bool> {
        let optional_result = condition.resolve(&mut ConditionResolverContext::new(self));

        if self.is_exiting_condition {
            return Option::None;
        }

        if let Some(result) = optional_result {
            self.resolved_conditions.insert(condition.get_unique_id(), result);
            self.restore_to_condition_save_point_if_necessary(&condition);
        }

        optional_result
    }

    fn restore_to_condition_save_point_if_necessary(&mut self, condition: &TCondition) {
        let optional_save_point = self.look_ahead_condition_save_points.remove(&condition.get_unique_id());
        if let Some(save_point) = optional_save_point {
            self.update_state_to_save_point(save_point, false);
            self.is_exiting_condition = true;
        }
    }

    fn handle_string(&mut self, text: &Rc<TString>) {
        if self.is_testing {
            self.validate_string(text);
        }

        if self.possible_new_line_save_point.is_some() && self.is_above_max_width(text.get_length() as u32) {
            let save_point = mem::replace(&mut self.possible_new_line_save_point, Option::None);
            self.update_state_to_save_point(save_point.unwrap(), true);
        } else {
            self.writer.write(&text);
        }
    }

    fn validate_string(&self, text: &Rc<TString>) {
        if !self.is_testing {
            panic!("Don't call this method unless self.is_testing is true.");
        }

        // This is possibly very slow (ex. could be a JS utf16 string that gets encoded to a rust utf8 string)
        let text_as_string = text.get_text_clone();
        if text_as_string.contains("\t") {
            panic!("Found a tab in the string. Before sending the string to the printer it needs to be broken up and the tab sent as a PrintItem::Tab. {0}", text_as_string);
        }
        if text_as_string.contains("\n") {
            panic!("Found a newline in the string. Before sending the string to the printer it needs to be broken up and the newline sent as a PrintItem::NewLine. {0}", text_as_string);
        }
    }

    /*#[allow(dead_code)]
    fn log_writer_for_debugging(&self) {
        let current_text = self.writer.to_string();

        println!("----");
        println!("{}", current_text);
    }*/
}
