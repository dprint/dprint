use super::WriteItem;
use super::print_items::*;
use super::writer::*;
use super::collections::{FastCellMap};
use super::get_write_items::{GetWriteItemsOptions};
use std::collections::HashMap;
use std::mem::{self, MaybeUninit};
use std::rc::Rc;

struct SavePoint<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    /// Name for debugging purposes.
    pub name: &'static str,
    pub new_line_group_depth: u16,
    pub writer_state: WriterState<TString>,
    pub possible_new_line_save_point: Option<Rc<SavePoint<TString, TInfo, TCondition>>>,
    pub node: Option<PrintItemPath<TString, TInfo, TCondition>>,
    pub look_ahead_condition_save_points: HashMap<usize, Rc<SavePoint<TString, TInfo, TCondition>>>,
    pub look_ahead_info_save_points: HashMap<usize, Rc<SavePoint<TString, TInfo, TCondition>>>,
    pub next_node_stack: Vec<Option<PrintItemPath<TString, TInfo, TCondition>>>,
}

struct PrintItemContainer<'a, TString, TInfo, TCondition> where TString : StringTrait, TInfo: InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    items: &'a Vec<PrintItem<TString, TInfo, TCondition>>,
    index: i32,
}

impl<'a, TString, TInfo, TCondition> Clone for PrintItemContainer<'a, TString, TInfo, TCondition> where TString : StringTrait, TInfo: InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    fn clone(&self) -> PrintItemContainer<'a, TString, TInfo, TCondition> {
        PrintItemContainer {
            items: self.items,
            index: self.index,
        }
    }
}

pub struct Printer<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    possible_new_line_save_point: Option<Rc<SavePoint<TString, TInfo, TCondition>>>,
    new_line_group_depth: u16,
    current_node: Option<PrintItemPath<TString, TInfo, TCondition>>,
    writer: Writer<TString>,
    resolved_conditions: HashMap<usize, Option<bool>>,
    resolved_infos: HashMap<usize, WriterInfo>,
    look_ahead_condition_save_points: HashMap<usize, Rc<SavePoint<TString, TInfo, TCondition>>>,
    look_ahead_info_save_points: FastCellMap<usize, SavePoint<TString, TInfo, TCondition>>,
    next_node_stack: Vec<Option<PrintItemPath<TString, TInfo, TCondition>>>,
    conditions_for_infos: HashMap<usize, HashMap<usize, (Rc<TCondition>, Rc<SavePoint<TString, TInfo, TCondition>>)>>,
    max_width: u32,
    skip_moving_next: bool,
    is_testing: bool, // todo: compiler directives
}

impl<'a, TString, TInfo, TCondition> Printer<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub fn new(start_node: Option<PrintItemPath<TString, TInfo, TCondition>>, options: GetWriteItemsOptions) -> Printer<TString, TInfo, TCondition> {
        Printer {
            possible_new_line_save_point: None,
            new_line_group_depth: 0,
            current_node: start_node,
            writer: Writer::new(WriterOptions {
                indent_width: options.indent_width,
            }),
            resolved_conditions: HashMap::new(),
            resolved_infos: HashMap::new(),
            look_ahead_condition_save_points: HashMap::new(),
            look_ahead_info_save_points: FastCellMap::new(),
            conditions_for_infos: HashMap::new(),
            next_node_stack: Vec::new(),
            max_width: options.max_width,
            skip_moving_next: false,
            is_testing: options.is_testing,
        }
    }

    /// Turns the print items into a collection of writer items according to the options.
    pub fn print(mut self) -> impl Iterator<Item = WriteItem<TString>> {
        while let Some(current_node) = &self.current_node {
            let current_node = unsafe { &*current_node.get_node() }; // ok because values won't be mutated while printing
            self.handle_print_node(current_node);

            if self.skip_moving_next {
                self.skip_moving_next = false;
            } else if let Some(current_node) = self.current_node {
                self.current_node = current_node.get_next();
            }

            while self.current_node.is_none() && !self.next_node_stack.is_empty() {
                self.current_node = self.next_node_stack.pop().flatten();
            }
        }

        if self.is_testing { self.verify_no_look_ahead_save_points(); }

        let writer = mem::replace(&mut self.writer, unsafe { MaybeUninit::zeroed().assume_init() });
        mem::drop(self);
        writer.get_items()
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

    pub fn get_resolved_info(&self, info: &TInfo) -> Option<&WriterInfo> {
        let resolved_info = self.resolved_infos.get(&info.get_unique_id());
        if resolved_info.is_none() && !self.look_ahead_info_save_points.contains_key(&info.get_unique_id()) {
            let save_point = self.create_save_point_for_restoring_condition(&info.get_name());
            self.look_ahead_info_save_points.insert(info.get_unique_id(), save_point);
        }

        resolved_info
    }

    pub fn get_resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
        if !self.resolved_conditions.contains_key(&condition_reference.id) && !self.look_ahead_condition_save_points.contains_key(&condition_reference.id) {
            let save_point = self.create_save_point_for_restoring_condition(&condition_reference.name);
            self.look_ahead_condition_save_points.insert(condition_reference.id, save_point);
        }

        let result = self.resolved_conditions.get(&condition_reference.id)?;
        result.map(|x| x.to_owned())
    }

    #[inline]
    fn handle_print_node(&mut self, print_node: &'a PrintNode<TString, TInfo, TCondition>) {
        match &print_node.item {
            PrintItem::String(text) => self.handle_string(text),
            PrintItem::Condition(condition) => self.handle_condition(condition, &print_node.next),
            PrintItem::Info(info) => self.handle_info(info),
            PrintItem::Signal(signal) => self.handle_signal(signal),
            PrintItem::RcPath(rc_path) => self.handle_rc_path(rc_path, &print_node.next),
        }
    }

    fn write_new_line(&mut self) {
        self.writer.new_line();
        self.possible_new_line_save_point = None;
    }

    fn create_save_point(&self, name: &'static str, next_node: Option<PrintItemPath<TString, TInfo, TCondition>>) -> Rc<SavePoint<TString, TInfo, TCondition>> {
        Rc::new(SavePoint {
            name,
            possible_new_line_save_point: self.possible_new_line_save_point.clone(),
            new_line_group_depth: self.new_line_group_depth,
            node: next_node,
            writer_state: self.writer.get_state(),
            look_ahead_condition_save_points: self.look_ahead_condition_save_points.clone(),
            look_ahead_info_save_points: self.look_ahead_info_save_points.clone_map(),
            next_node_stack: self.next_node_stack.clone(),
        })
    }

    fn create_save_point_for_restoring_condition(&self, name: &'static str) -> Rc<SavePoint<TString, TInfo, TCondition>> {
        self.create_save_point(name, self.current_node.clone())
    }

    fn mark_possible_new_line_if_able(&mut self) {
        if let Some(new_line_save_point) = &self.possible_new_line_save_point {
            if self.new_line_group_depth > new_line_save_point.new_line_group_depth {
                return;
            }
        }

        let next_node = self.current_node.as_ref().unwrap().get_next();
        self.possible_new_line_save_point = Some(self.create_save_point("newline", next_node));
    }

    fn is_above_max_width(&self, offset: u32) -> bool {
        self.writer.get_line_column() + 1 + offset > self.max_width
    }

    fn update_state_to_save_point(&mut self, save_point: Rc<SavePoint<TString, TInfo, TCondition>>, is_for_new_line: bool) {
        match Rc::try_unwrap(save_point) {
            Ok(save_point) => {
                self.writer.set_state(save_point.writer_state);
                self.possible_new_line_save_point = if is_for_new_line { None } else { save_point.possible_new_line_save_point };
                self.current_node = save_point.node;
                self.new_line_group_depth = save_point.new_line_group_depth;
                self.look_ahead_condition_save_points = save_point.look_ahead_condition_save_points;
                self.look_ahead_info_save_points.replace_map(save_point.look_ahead_info_save_points);
                self.next_node_stack = save_point.next_node_stack;
            },
            Err(save_point) => {
                self.writer.set_state(save_point.writer_state.clone());
                self.possible_new_line_save_point = if is_for_new_line { None } else { save_point.possible_new_line_save_point.clone() };
                self.current_node = save_point.node.clone();
                self.new_line_group_depth = save_point.new_line_group_depth;
                self.look_ahead_condition_save_points = save_point.look_ahead_condition_save_points.clone();
                self.look_ahead_info_save_points.replace_map(save_point.look_ahead_info_save_points.clone());
                self.next_node_stack = save_point.next_node_stack.clone();
            }
        }

        if is_for_new_line {
            self.write_new_line();
        }

        self.skip_moving_next = true;
    }

    #[inline]
    fn handle_signal(&mut self, signal: &Signal) {
        match signal {
            Signal::NewLine => self.write_new_line(),
            Signal::Tab => self.writer.tab(),
            Signal::ExpectNewLine => {
                self.writer.mark_expect_new_line();
                self.possible_new_line_save_point = None;
            }
            Signal::PossibleNewLine => self.mark_possible_new_line_if_able(),
            Signal::SpaceOrNewLine => {
                if self.is_above_max_width(1) {
                    let optional_save_state = mem::replace(&mut self.possible_new_line_save_point, None);
                    if optional_save_state.is_none() {
                        self.write_new_line();
                    } else if let Some(save_state) = optional_save_state {
                        if save_state.new_line_group_depth >= self.new_line_group_depth {
                            self.write_new_line();
                        } else {
                            self.update_state_to_save_point(save_state, true);
                            return;
                        }
                    }
                } else {
                    self.mark_possible_new_line_if_able();
                    self.writer.space();
                }
            }
            Signal::StartIndent => self.writer.start_indent(),
            Signal::FinishIndent => self.writer.finish_indent(),
            Signal::StartNewLineGroup => self.new_line_group_depth += 1,
            Signal::FinishNewLineGroup => self.new_line_group_depth -= 1,
            Signal::SingleIndent => self.writer.single_indent(),
            Signal::StartIgnoringIndent => self.writer.start_ignoring_indent(),
            Signal::FinishIgnoringIndent => self.writer.finish_ignoring_indent(),
        }
    }

    #[inline]
    fn handle_info(&mut self, info: &TInfo) {
        let info_id = info.get_unique_id();
        self.resolved_infos.insert(info_id, self.get_writer_info());
        let option_save_point = self.look_ahead_info_save_points.remove(&info_id);
        if let Some(save_point) = option_save_point {
            self.update_state_to_save_point(save_point, false);
            return;
        }

        // check if there are any conditions that should be re-evaluated based on this info update
        if self.conditions_for_infos.contains_key(&info_id) {
            // todo: avoid this clone
            let conditions_for_info = self.conditions_for_infos.get(&info_id).unwrap().clone();
            for (condition, save_point) in conditions_for_info.values() {
                let condition_id = condition.get_unique_id();
                if let Some(resolved_condition_value) = self.resolved_conditions.get(&condition_id).map(|x| x.to_owned()).flatten() {
                    // todo: this should definitely not use the condition context because the printer is not on the condition
                    if let Some(condition_value) = condition.resolve(&mut ConditionResolverContext::new(self)) {
                        if condition_value != resolved_condition_value {
                            self.update_state_to_save_point(save_point.clone(), false);
                            return;
                        }
                    }
                }
            }
        }
    }

    #[inline]
    fn handle_condition(&mut self, condition: &'a Rc<TCondition>, next_node: &Option<PrintItemPath<TString, TInfo, TCondition>>) {
        let condition_id = condition.get_unique_id();
        if let Some(dependent_infos) = condition.get_dependent_infos() {
            for info in dependent_infos {
                let info_id = info.get_unique_id();
                let save_point = self.create_save_point_for_restoring_condition(condition.get_name());
                let conditions_for_info = if let Some(conditions) = self.conditions_for_infos.get_mut(&info_id) {
                    conditions
                } else {
                    self.conditions_for_infos.insert(info_id, HashMap::new());
                    self.conditions_for_infos.get_mut(&info_id).unwrap()
                };

                let condition_id = condition.get_unique_id();
                conditions_for_info.insert(condition_id, (condition.clone(), save_point));
            }
        }

        let condition_value = condition.resolve(&mut ConditionResolverContext::new(self));
        if condition.get_is_stored() {
            self.resolved_conditions.insert(condition_id, condition_value);
        }

        let save_point = self.look_ahead_condition_save_points.get(&condition_id);
        if condition_value.is_some() && save_point.is_some() {
            let save_point = self.look_ahead_condition_save_points.remove(&condition_id);
            self.update_state_to_save_point(save_point.unwrap(), false);
            return;
        }

        if condition_value.is_some() && condition_value.unwrap() {
            if let Some(true_path) = condition.get_true_path() {
                self.current_node = Some(true_path.clone());
                self.next_node_stack.push(next_node.clone());
                self.skip_moving_next = true;
            }
        } else {
            if let Some(false_path) = condition.get_false_path() {
                self.current_node = Some(false_path.clone());
                self.next_node_stack.push(next_node.clone());
                self.skip_moving_next = true;
            }
        }
    }

    #[inline]
    fn handle_rc_path(&mut self, print_item_path: &PrintItemPath<TString, TInfo, TCondition>, next_node: &Option<PrintItemPath<TString, TInfo, TCondition>>) {
        self.next_node_stack.push(next_node.clone());
        self.current_node = Some(print_item_path.clone());
        self.skip_moving_next = true;
    }

    #[inline]
    fn handle_string(&mut self, text: &Rc<StringContainer<TString>>) {
        if self.is_testing {
            self.validate_string(&text.text);
        }

        if self.possible_new_line_save_point.is_some() && self.is_above_max_width(text.char_count) {
            let save_point = mem::replace(&mut self.possible_new_line_save_point, Option::None);
            self.update_state_to_save_point(save_point.unwrap(), true);
        } else {
            self.writer.write(text.clone());
        }
    }

    fn validate_string(&self, text: &TString) {
        if !self.is_testing {
            panic!("Don't call this method unless self.is_testing is true.");
        }

        let text_as_string = text.get_text();
        if text_as_string.contains("\t") {
            panic!("Found a tab in the string. Before sending the string to the printer it needs to be broken up and the tab sent as a PrintItem::Tab. {0}", text_as_string);
        }
        if text_as_string.contains("\n") {
            panic!("Found a newline in the string. Before sending the string to the printer it needs to be broken up and the newline sent as a PrintItem::NewLine. {0}", text_as_string);
        }
    }

    fn verify_no_look_ahead_save_points(&self) {
        // The look ahead save points should be empty when printing is finished. If it's not
        // then that indicates that the parser tried to resolve a condition or info that was
        // never added to the print items. In this scenario, the look ahead hash maps will
        // be cloned when creating a save point and contain items that don't need to exist
        // in them thus having an unnecessary performance impact.
        if let Some((_, save_point)) = self.look_ahead_condition_save_points.iter().next() {
            self.panic_for_save_point_existing(save_point)
        }
        if let Some(save_point) = self.look_ahead_info_save_points.get_any_item() {
            self.panic_for_save_point_existing(&save_point)
        }
    }

    fn panic_for_save_point_existing(&self, save_point: &SavePoint<TString, TInfo, TCondition>) {
        panic!(
            concat!(
                "'{}' was never added to the print items in this scenario. This can ",
                "have slight performance implications in large files."
            ),
            save_point.name
        );
    }
}
