use bumpalo::Bump;
use rustc_hash::FxHashMap;

use super::collections::*;
use super::infinite_reevaluation_protection::InfiniteReevaluationProtector;
use super::print_items::*;
use super::thread_state;
use super::writer::*;
use super::WriteItem;

struct SavePoint<'a> {
  #[cfg(debug_assertions)]
  /// Name for debugging purposes.
  pub name: &'static str,
  pub new_line_group_depth: u16,
  pub force_no_newlines_depth: u8,
  pub writer_state: WriterState<'a>,
  pub possible_new_line_save_point: Option<&'a SavePoint<'a>>,
  pub node: Option<PrintItemPath>,
  pub look_ahead_condition_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_line_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_column_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_is_start_of_line_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_indent_level_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_line_start_column_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub look_ahead_line_start_indent_level_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  pub next_node_stack: RcStack,
}

struct PrintItemContainer<'a> {
  items: &'a Vec<PrintItem>,
  index: i32,
}

impl<'a> Clone for PrintItemContainer<'a> {
  fn clone(&self) -> PrintItemContainer<'a> {
    PrintItemContainer {
      items: self.items,
      index: self.index,
    }
  }
}

#[cfg(feature = "tracing")]
pub struct PrintTracingResult<'a> {
  pub traces: Vec<Trace>,
  pub writer_nodes: Vec<&'a GraphNode<'a, WriteItem<'a>>>,
}

/// Options for printing.
pub struct PrinterOptions {
  /// The width the printer will attempt to keep the line under.
  pub max_width: u32,
  /// The number of columns to count when indenting or using a tab.
  pub indent_width: u8,
  #[cfg(feature = "tracing")]
  pub enable_tracing: bool,
}

pub struct Printer<'a> {
  bump: &'a Bump,
  possible_new_line_save_point: Option<&'a SavePoint<'a>>,
  new_line_group_depth: u16,
  force_no_newlines_depth: u8,
  current_node: Option<PrintItemPath>,
  writer: Writer<'a>,
  // Use a regular hash map here because only some conditions are stored (not all).
  resolved_conditions: FxHashMap<u32, Option<bool>>,
  // Use these "VecU32Map" for resolved infos because it has much faster
  // lookups than a hash map and generally infos seem to be resolved
  // about 90% of the time, so the extra memory usage is probably not
  // a big deal.
  resolved_line_number_anchors: VecU32U32Map,
  resolved_line_numbers: VecU32U32Map,
  resolved_column_numbers: VecU32U32Map,
  resolved_is_start_of_lines: VecU32BoolMap,
  resolved_indent_levels: VecU32U8Map,
  resolved_line_start_column_numbers: VecU32U32Map,
  resolved_line_start_indent_levels: VecU32U8Map,
  look_ahead_condition_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_line_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_column_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_is_start_of_line_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_indent_level_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_line_start_column_number_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  look_ahead_line_start_indent_level_save_points: FxHashMap<u32, &'a SavePoint<'a>>,
  infinite_reevaluation_protector: InfiniteReevaluationProtector,
  next_node_stack: RcStack,
  stored_condition_save_points: FxHashMap<u32, (&'a Condition, &'a SavePoint<'a>)>,
  max_width: u32,
  skip_moving_next: bool,
  resolving_save_point: Option<&'a SavePoint<'a>>,
  #[cfg(feature = "tracing")]
  traces: Option<Vec<Trace>>,
  #[cfg(feature = "tracing")]
  start_time: std::time::Instant,
}

impl<'a> Printer<'a> {
  pub fn new(bump: &'a Bump, start_node: Option<PrintItemPath>, options: PrinterOptions) -> Printer<'a> {
    Printer {
      bump,
      possible_new_line_save_point: None,
      new_line_group_depth: 0,
      force_no_newlines_depth: 0,
      current_node: start_node,
      writer: Writer::new(
        bump,
        WriterOptions {
          indent_width: options.indent_width,
          #[cfg(feature = "tracing")]
          enable_tracing: options.enable_tracing,
        },
      ),
      resolved_conditions: FxHashMap::default(),
      resolved_line_number_anchors: VecU32U32Map::with_capacity(thread_state::next_line_number_anchor_id()),
      resolved_line_numbers: VecU32U32Map::with_capacity(thread_state::next_line_number_id()),
      resolved_column_numbers: VecU32U32Map::with_capacity(thread_state::next_column_number_id()),
      resolved_is_start_of_lines: VecU32BoolMap::with_capacity(thread_state::next_is_start_of_line_id()),
      resolved_indent_levels: VecU32U8Map::with_capacity(thread_state::next_indent_level_id()),
      resolved_line_start_column_numbers: VecU32U32Map::with_capacity(thread_state::next_line_start_column_number_id()),
      resolved_line_start_indent_levels: VecU32U8Map::with_capacity(thread_state::next_line_start_indent_level_id()),
      look_ahead_condition_save_points: FxHashMap::default(),
      look_ahead_line_number_save_points: FxHashMap::default(),
      look_ahead_column_number_save_points: FxHashMap::default(),
      look_ahead_is_start_of_line_save_points: FxHashMap::default(),
      look_ahead_indent_level_save_points: FxHashMap::default(),
      look_ahead_line_start_column_number_save_points: FxHashMap::default(),
      look_ahead_line_start_indent_level_save_points: FxHashMap::default(),
      infinite_reevaluation_protector: InfiniteReevaluationProtector::with_capacity(thread_state::next_condition_reevaluation_id()),
      stored_condition_save_points: FxHashMap::default(),
      next_node_stack: RcStack::default(),
      max_width: options.max_width,
      skip_moving_next: false,
      resolving_save_point: None,
      #[cfg(feature = "tracing")]
      traces: if options.enable_tracing { Some(Vec::new()) } else { None },
      #[cfg(feature = "tracing")]
      start_time: std::time::Instant::now(),
    }
  }

  /// Turns the print items into a collection of writer items according to the options.
  pub fn print(mut self) -> Option<impl Iterator<Item = WriteItem<'a>>> {
    self.inner_print();
    self.writer.items()
  }

  /// Turns the print items into a collection of writer items according to the options along with traces.
  #[cfg(feature = "tracing")]
  pub fn print_for_tracing(mut self) -> PrintTracingResult<'a> {
    self.inner_print();

    PrintTracingResult {
      traces: self.traces.expect("Should have set enable_tracing to true when creating the printer."),
      writer_nodes: self.writer.nodes(),
    }
  }

  fn inner_print(&mut self) {
    while let Some(current_node) = &self.current_node {
      let current_node = unsafe { &*current_node.get_node() }; // ok because values won't be mutated while printing
      self.handle_print_node(current_node);

      #[cfg(feature = "tracing")]
      self.create_trace(current_node);

      // println!("{}", self.writer.to_string_for_debugging());

      if self.skip_moving_next {
        self.skip_moving_next = false;
      } else if let Some(current_node) = self.current_node {
        self.current_node = current_node.get_next();
      }

      while self.current_node.is_none() && !self.next_node_stack.is_empty() {
        self.current_node = self.next_node_stack.pop();
      }
    }

    #[cfg(debug_assertions)]
    self.verify_no_look_ahead_save_points();
    #[cfg(debug_assertions)]
    self.ensure_counts_zero();
  }

  #[cfg(feature = "tracing")]
  fn create_trace(&mut self, current_node: &PrintNode) {
    if let Some(traces) = self.traces.as_mut() {
      traces.push(Trace {
        nanos: (std::time::Instant::now() - self.start_time).as_nanos(),
        print_node_id: current_node.print_node_id,
        writer_node_id: self.writer.current_node_id(),
      });
    }
  }

  #[inline]
  pub fn get_writer_info(&self) -> WriterInfo {
    self.writer.writer_info()
  }

  pub fn resolved_line_number(&mut self, line_number: LineNumber) -> Option<u32> {
    let resolved_number = self.resolved_line_numbers.get(line_number.unique_id());
    if resolved_number.is_none() && !self.look_ahead_line_number_save_points.contains_key(&line_number.unique_id()) {
      let save_point = self.get_save_point_for_restoring_condition(line_number.name());
      self.look_ahead_line_number_save_points.insert(line_number.unique_id(), save_point);
    }

    resolved_number
  }

  pub fn resolved_column_number(&mut self, column_number: ColumnNumber) -> Option<u32> {
    let resolved_number = self.resolved_column_numbers.get(column_number.unique_id());
    if resolved_number.is_none() && !self.look_ahead_column_number_save_points.contains_key(&column_number.unique_id()) {
      let save_point = self.get_save_point_for_restoring_condition(column_number.name());
      self.look_ahead_column_number_save_points.insert(column_number.unique_id(), save_point);
    }

    resolved_number
  }

  pub fn resolved_is_start_of_line(&mut self, is_start_of_line: IsStartOfLine) -> Option<bool> {
    let resolved_is_start_of_line = self.resolved_is_start_of_lines.get(is_start_of_line.unique_id());
    if resolved_is_start_of_line.is_none() && !self.look_ahead_is_start_of_line_save_points.contains_key(&is_start_of_line.unique_id()) {
      let save_point = self.get_save_point_for_restoring_condition(is_start_of_line.name());
      self.look_ahead_is_start_of_line_save_points.insert(is_start_of_line.unique_id(), save_point);
    }

    resolved_is_start_of_line
  }

  pub fn resolved_indent_level(&mut self, indent_level: IndentLevel) -> Option<u8> {
    let resolved_indent_level = self.resolved_indent_levels.get(indent_level.unique_id());
    if resolved_indent_level.is_none() && !self.look_ahead_indent_level_save_points.contains_key(&indent_level.unique_id()) {
      let save_point = self.get_save_point_for_restoring_condition(indent_level.name());
      self.look_ahead_indent_level_save_points.insert(indent_level.unique_id(), save_point);
    }

    resolved_indent_level
  }

  pub fn resolved_line_start_column_number(&mut self, line_start_column_number: LineStartColumnNumber) -> Option<u32> {
    let resolved_line_start_column_number = self.resolved_line_start_column_numbers.get(line_start_column_number.unique_id());
    if resolved_line_start_column_number.is_none()
      && !self
        .look_ahead_line_start_column_number_save_points
        .contains_key(&line_start_column_number.unique_id())
    {
      let save_point = self.get_save_point_for_restoring_condition(line_start_column_number.name());
      self
        .look_ahead_line_start_column_number_save_points
        .insert(line_start_column_number.unique_id(), save_point);
    }

    resolved_line_start_column_number
  }

  pub fn resolved_line_start_indent_level(&mut self, line_start_indent_level: LineStartIndentLevel) -> Option<u8> {
    let resolved_line_start_indent_level = self.resolved_line_start_indent_levels.get(line_start_indent_level.unique_id());
    if resolved_line_start_indent_level.is_none()
      && !self
        .look_ahead_line_start_indent_level_save_points
        .contains_key(&line_start_indent_level.unique_id())
    {
      let save_point = self.get_save_point_for_restoring_condition(line_start_indent_level.name());
      self
        .look_ahead_line_start_indent_level_save_points
        .insert(line_start_indent_level.unique_id(), save_point);
    }

    resolved_line_start_indent_level
  }

  pub fn clear_info(&mut self, info: Info) {
    match info {
      Info::LineNumber(info) => self.resolved_line_numbers.remove(info.unique_id()),
      Info::ColumnNumber(info) => self.resolved_column_numbers.remove(info.unique_id()),
      Info::IsStartOfLine(info) => self.resolved_is_start_of_lines.remove(info.unique_id()),
      Info::IndentLevel(info) => self.resolved_indent_levels.remove(info.unique_id()),
      Info::LineStartColumnNumber(info) => self.resolved_line_start_column_numbers.remove(info.unique_id()),
      Info::LineStartIndentLevel(info) => self.resolved_line_start_indent_levels.remove(info.unique_id()),
    }
  }

  pub fn resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
    if !self.resolved_conditions.contains_key(&condition_reference.id) && !self.look_ahead_condition_save_points.contains_key(&condition_reference.id) {
      let save_point = self.get_save_point_for_restoring_condition(condition_reference.name());
      self.look_ahead_condition_save_points.insert(condition_reference.id, save_point);
    }

    let result = self.resolved_conditions.get(&condition_reference.id)?;
    result.map(|x| x.to_owned())
  }

  pub fn is_forcing_no_newlines(&self) -> bool {
    self.force_no_newlines_depth > 0
  }

  #[inline]
  fn handle_print_node(&mut self, print_node: &PrintNode) {
    match &print_node.item {
      PrintItem::String(text) => self.handle_string(text),
      PrintItem::Condition(condition) => self.handle_condition(condition, &print_node.next),
      PrintItem::Signal(signal) => self.handle_signal(signal),
      PrintItem::RcPath(rc_path) => self.handle_rc_path(rc_path, &print_node.next),
      PrintItem::Anchor(anchor) => self.handle_anchor(anchor),
      PrintItem::Info(info) => self.handle_targeted_info(info),
      PrintItem::ConditionReevaluation(reevaluation) => self.handle_condition_reevaluation(reevaluation),
    }
  }

  fn write_new_line(&mut self) {
    self.writer.new_line();
    self.possible_new_line_save_point = None;
  }

  fn create_save_point(&self, _name: &'static str, next_node: Option<PrintItemPath>) -> &'a SavePoint<'a> {
    self.bump.alloc(SavePoint {
      #[cfg(debug_assertions)]
      name: _name,
      possible_new_line_save_point: self.possible_new_line_save_point,
      new_line_group_depth: self.new_line_group_depth,
      force_no_newlines_depth: self.force_no_newlines_depth,
      node: next_node,
      writer_state: self.writer.state(),
      look_ahead_condition_save_points: self.look_ahead_condition_save_points.clone(),
      look_ahead_line_number_save_points: self.look_ahead_line_number_save_points.clone(),
      look_ahead_column_number_save_points: self.look_ahead_column_number_save_points.clone(),
      look_ahead_is_start_of_line_save_points: self.look_ahead_is_start_of_line_save_points.clone(),
      look_ahead_indent_level_save_points: self.look_ahead_indent_level_save_points.clone(),
      look_ahead_line_start_column_number_save_points: self.look_ahead_line_start_column_number_save_points.clone(),
      look_ahead_line_start_indent_level_save_points: self.look_ahead_line_start_indent_level_save_points.clone(),
      next_node_stack: self.next_node_stack.clone(),
    })
  }

  #[inline]
  fn get_save_point_for_restoring_condition(&self, name: &'static str) -> &'a SavePoint<'a> {
    if let Some(save_point) = &self.resolving_save_point {
      save_point
    } else {
      self.create_save_point(name, self.current_node)
    }
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

  #[inline]
  fn is_above_max_width(&self, offset: u32) -> bool {
    self.writer.column_number() + offset > self.max_width
  }

  fn update_state_to_save_point(&mut self, save_point: &'a SavePoint<'a>, is_for_new_line: bool) {
    self.writer.set_state(save_point.writer_state.clone());
    self.possible_new_line_save_point = if is_for_new_line { None } else { save_point.possible_new_line_save_point };
    self.current_node = save_point.node;
    self.new_line_group_depth = save_point.new_line_group_depth;
    self.force_no_newlines_depth = save_point.force_no_newlines_depth;
    self.look_ahead_condition_save_points = save_point.look_ahead_condition_save_points.clone();
    self.look_ahead_line_number_save_points = save_point.look_ahead_line_number_save_points.clone();
    self.look_ahead_column_number_save_points = save_point.look_ahead_column_number_save_points.clone();
    self.look_ahead_is_start_of_line_save_points = save_point.look_ahead_is_start_of_line_save_points.clone();
    self.look_ahead_indent_level_save_points = save_point.look_ahead_indent_level_save_points.clone();
    self.look_ahead_line_start_column_number_save_points = save_point.look_ahead_line_start_column_number_save_points.clone();
    self.look_ahead_line_start_indent_level_save_points = save_point.look_ahead_line_start_indent_level_save_points.clone();
    self.next_node_stack = save_point.next_node_stack.clone();

    if is_for_new_line {
      self.write_new_line();
    }

    self.skip_moving_next = true;
  }

  #[inline]
  fn handle_signal(&mut self, signal: &Signal) {
    match signal {
      Signal::NewLine => {
        if self.allow_new_lines() {
          self.write_new_line()
        }
      }
      Signal::Tab => self.writer.tab(),
      Signal::ExpectNewLine => {
        // just always allow this for now since it's most likely a comment...
        self.writer.mark_expect_new_line();
        self.possible_new_line_save_point = None;
      }
      Signal::PossibleNewLine => {
        if self.allow_new_lines() {
          self.mark_possible_new_line_if_able()
        }
      }
      Signal::SpaceOrNewLine => {
        if self.allow_new_lines() {
          if self.is_above_max_width(1) {
            let optional_save_state = std::mem::replace(&mut self.possible_new_line_save_point, None);
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
            self.writer.space_if_not_trailing();
          }
        } else {
          self.writer.space_if_not_trailing();
        }
      }
      Signal::QueueStartIndent => self.writer.queue_indent(),
      Signal::StartIndent => self.writer.start_indent(),
      Signal::FinishIndent => self.writer.finish_indent(),
      Signal::StartNewLineGroup => self.new_line_group_depth += 1,
      Signal::FinishNewLineGroup => self.new_line_group_depth -= 1,
      Signal::SingleIndent => self.writer.single_indent(),
      Signal::StartIgnoringIndent => self.writer.start_ignoring_indent(),
      Signal::FinishIgnoringIndent => self.writer.finish_ignoring_indent(),
      Signal::StartForceNoNewLines => self.force_no_newlines_depth += 1,
      Signal::FinishForceNoNewLines => self.force_no_newlines_depth -= 1,
      Signal::SpaceIfNotTrailing => self.writer.space_if_not_trailing(),
    }
  }

  #[inline]
  fn handle_anchor(&mut self, anchor: &Anchor) {
    match anchor {
      Anchor::LineNumber(anchor) => {
        let id = anchor.unique_id();
        let current_line_number = self.writer.line_number();
        if let Some(past_line_number) = self.resolved_line_number_anchors.get(id) {
          let difference = (current_line_number as isize) - (past_line_number as isize);
          if difference != 0 {
            let line_number_id = anchor.line_number_id();
            if let Some(value) = self.resolved_line_numbers.get(line_number_id) {
              let new_value = ((value as isize) + difference) as u32;
              self.resolved_line_numbers.insert(line_number_id, new_value);
            }
          }
        }
        self.resolved_line_number_anchors.insert(id, current_line_number);
      }
    }
  }

  #[inline]
  fn handle_targeted_info(&mut self, info: &Info) {
    match info {
      Info::LineNumber(line_number) => {
        let line_number_id = line_number.unique_id();
        self.resolved_line_numbers.insert(line_number_id, self.writer.line_number());
        let option_save_point = self.look_ahead_line_number_save_points.remove(&line_number_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
      Info::ColumnNumber(column_number) => {
        let column_number_id = column_number.unique_id();
        self.resolved_column_numbers.insert(column_number_id, self.writer.column_number());
        let option_save_point = self.look_ahead_column_number_save_points.remove(&column_number_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
      Info::IsStartOfLine(is_start_of_line) => {
        let is_start_of_line_id = is_start_of_line.unique_id();
        self.resolved_is_start_of_lines.insert(is_start_of_line_id, self.writer.is_start_of_line());
        let option_save_point = self.look_ahead_is_start_of_line_save_points.remove(&is_start_of_line_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
      Info::IndentLevel(indent_level) => {
        let indent_level_id = indent_level.unique_id();
        self.resolved_indent_levels.insert(indent_level_id, self.writer.indent_level());
        let option_save_point = self.look_ahead_indent_level_save_points.remove(&indent_level_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
      Info::LineStartColumnNumber(line_start_column_number) => {
        let line_start_column_number_id = line_start_column_number.unique_id();
        self
          .resolved_line_start_column_numbers
          .insert(line_start_column_number_id, self.writer.line_start_column_number());
        let option_save_point = self.look_ahead_line_start_column_number_save_points.remove(&line_start_column_number_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
      Info::LineStartIndentLevel(line_start_indent_level) => {
        let line_start_indent_level_id = line_start_indent_level.unique_id();
        self
          .resolved_line_start_indent_levels
          .insert(line_start_indent_level_id, self.writer.line_start_indent_level());
        let option_save_point = self.look_ahead_line_start_indent_level_save_points.remove(&line_start_indent_level_id);
        if let Some(save_point) = option_save_point {
          self.update_state_to_save_point(save_point, false);
        }
      }
    }
  }

  #[inline]
  fn handle_condition_reevaluation(&mut self, condition_reevaluation: &ConditionReevaluation) {
    let condition_id = condition_reevaluation.condition_id;
    if let Some((condition, save_point)) = self.stored_condition_save_points.get(&condition_id).cloned() {
      if let Some(past_condition_value) = self.resolved_conditions.get(&condition_id).and_then(|x| x.to_owned()) {
        self.resolving_save_point.replace(save_point);
        let mut context = ConditionResolverContext::new(self, save_point.writer_state.writer_info(self.writer.indent_width()));
        let latest_condition_value = condition.resolve(&mut context);
        self.resolving_save_point.take();

        // Do not re-evaluate the condition if it's flipped back and forth a decent number of times.
        // If it hits the max number of times it can flip then an error will be logged.
        let should_reevaluate = self.infinite_reevaluation_protector.should_reevaluate(
          condition_reevaluation.condition_reevaluation_id,
          latest_condition_value,
          past_condition_value,
        );
        if should_reevaluate {
          if let Some(latest_condition_value) = latest_condition_value {
            if latest_condition_value != past_condition_value {
              self.update_state_to_save_point(save_point, false);
            }
          } else {
            self.resolved_conditions.remove(&condition_id);
          }
        }
      }
    }
  }

  #[inline]
  fn handle_condition(&mut self, condition: &'a Condition, next_node: &Option<PrintItemPath>) {
    let condition_id = condition.unique_id();

    if condition.store_save_point {
      let save_point = self.get_save_point_for_restoring_condition(condition.name());
      self.stored_condition_save_points.insert(condition.unique_id(), (condition, save_point));
    }

    let condition_value = condition.resolve(&mut ConditionResolverContext::new(self, self.get_writer_info()));
    if condition.is_stored {
      self.resolved_conditions.insert(condition_id, condition_value);
    }

    let save_point = self.look_ahead_condition_save_points.get(&condition_id);
    if condition_value.is_some() && save_point.is_some() {
      let save_point = self.look_ahead_condition_save_points.remove(&condition_id);
      self.update_state_to_save_point(save_point.unwrap(), false);
      return;
    }

    if condition_value.is_some() && condition_value.unwrap() {
      if let Some(true_path) = condition.true_path {
        self.current_node = Some(true_path);
        if let Some(path) = next_node {
          self.next_node_stack.push(path);
        }
        self.skip_moving_next = true;
      }
    } else if let Some(false_path) = condition.false_path {
      self.current_node = Some(false_path);
      if let Some(path) = next_node {
        self.next_node_stack.push(path);
      }
      self.skip_moving_next = true;
    }
  }

  #[inline]
  fn handle_rc_path(&mut self, print_item_path: &PrintItemPath, next_node: &Option<PrintItemPath>) {
    if let Some(path) = next_node {
      self.next_node_stack.push(path);
    }
    self.current_node = Some(print_item_path);
    self.skip_moving_next = true;
  }

  #[inline]
  fn handle_string(&mut self, text: &'a StringContainer) {
    #[cfg(debug_assertions)]
    self.validate_string(&text.text);

    if self.possible_new_line_save_point.is_some() && self.is_above_max_width(text.char_count) && self.allow_new_lines() {
      let save_point = std::mem::replace(&mut self.possible_new_line_save_point, Option::None);
      self.update_state_to_save_point(save_point.unwrap(), true);
    } else {
      self.writer.write(text);
    }
  }

  #[inline]
  fn allow_new_lines(&self) -> bool {
    self.force_no_newlines_depth == 0
  }

  #[cfg(debug_assertions)]
  fn validate_string(&self, text: &str) {
    // The ir_helpers::gen_from_raw_string(...) helper function might be useful if you get either of these panics.
    if text.contains('\t') {
      panic!(
        "Debug panic! Found a tab in the string. Before sending the string to the printer it needs to be broken up and the tab sent as a PrintItem::Tab. {0}",
        text
      );
    }
    if text.contains('\n') {
      panic!("Debug panic! Found a newline in the string. Before sending the string to the printer it needs to be broken up and the newline sent as a PrintItem::NewLine. {0}", text);
    }
  }

  #[cfg(debug_assertions)]
  fn verify_no_look_ahead_save_points(&self) {
    // The look ahead save points should be empty when printing is finished. If it's not
    // then that indicates that the generator tried to resolve a condition or info that was
    // never added to the print items. In this scenario, the look ahead hash maps will
    // be cloned when creating a save point and contain items that don't need to exist
    // in them thus having an unnecessary performance impact.
    let save_point = self
      .look_ahead_condition_save_points
      .values()
      .next()
      .or_else(|| self.look_ahead_line_number_save_points.values().next())
      .or_else(|| self.look_ahead_column_number_save_points.values().next())
      .or_else(|| self.look_ahead_is_start_of_line_save_points.values().next())
      .or_else(|| self.look_ahead_indent_level_save_points.values().next())
      .or_else(|| self.look_ahead_line_start_column_number_save_points.values().next())
      .or_else(|| self.look_ahead_line_start_indent_level_save_points.values().next());
    if let Some(save_point) = save_point {
      self.panic_for_save_point_existing(save_point)
    }
  }

  #[cfg(debug_assertions)]
  fn panic_for_save_point_existing(&self, save_point: &SavePoint<'a>) {
    panic!(
      concat!(
        "Debug panic! '{}' was never added to the print items in this scenario. This can ",
        "have slight performance implications in large files."
      ),
      save_point.name
    );
  }

  #[cfg(debug_assertions)]
  fn ensure_counts_zero(&self) {
    if self.new_line_group_depth != 0 {
      panic!(
        "Debug panic! The new line group depth was not zero after printing. {0}",
        self.new_line_group_depth
      );
    }
    if self.force_no_newlines_depth != 0 {
      panic!(
        "Debug panic! The force no newlines depth was not zero after printing. {0}",
        self.force_no_newlines_depth
      );
    }
    if self.writer.indentation_level() != 0 {
      panic!(
        "Debug panic! The writer indentation level was not zero after printing. {0}",
        self.writer.indentation_level()
      );
    }
    if self.writer.ignore_indent_count() != 0 {
      panic!(
        "Debug panic! The writer ignore indent count was not zero after printing. {0}",
        self.writer.ignore_indent_count()
      );
    }
  }
}
