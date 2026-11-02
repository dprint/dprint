use std::hash::BuildHasherDefault;

type BumpHashMap<'a, K, V> = hashbrown::HashMap<K, V, BuildHasherDefault<rustc_hash::FxHasher>, &'a bumpalo::Bump>;

use super::Arena;
use super::WriteItem;
use super::collections::*;
use super::infinite_reevaluation_protection::InfiniteReevaluationProtector;
use super::print_items::*;
use super::thread_state;
use super::writer::*;

pub struct SavePoint<'a> {
  #[cfg(debug_assertions)]
  /// Name for debugging purposes.
  pub name: &'static str,
  pub new_line_group_depth: u16,
  pub force_no_newlines_depth: u8,
  pub writer_state: WriterState<'a>,
  pub possible_new_line_save_point: Option<&'a SavePoint<'a>>,
  pub node: Option<PrintItemPath<'a>>,
  pub look_ahead_save_points: LookAheadSavePoints<'a>,
  pub next_node_stack: NodeStack<'a>,
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
  arena: &'a Arena,
  possible_new_line_save_point: Option<&'a SavePoint<'a>>,
  new_line_group_depth: u16,
  force_no_newlines_depth: u8,
  current_node: Option<PrintItemPath<'a>>,
  writer: Writer<'a>,
  // Use a hash map here because only some conditions are stored (not all).
  resolved_conditions: BumpHashMap<'a, u32, Option<bool>>,
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
  look_ahead_save_points: LookAheadSavePoints<'a>,
  infinite_reevaluation_protector: InfiniteReevaluationProtector,
  next_node_stack: NodeStack<'a>,
  stored_condition_save_points: BumpHashMap<'a, u32, (&'a Condition<'a>, &'a SavePoint<'a>)>,
  max_width: u32,
  skip_moving_next: bool,
  resolving_save_point: Option<&'a SavePoint<'a>>,
  /// The measurement in progress of whether print items fit on the rest of the line.
  measurement: Option<Measurement<'a>>,
  /// The measurement the condition being resolved asked for.
  requested_measurement: Option<MeasurementRequest>,
  /// Whether print items fit on the line by the line number measured up to.
  measurement_results: BumpHashMap<'a, u32, MeasurementResult>,
  #[cfg(feature = "tracing")]
  traces: Option<Vec<Trace>>,
  /// Why the printer went back to an earlier point while printing the current node.
  #[cfg(feature = "tracing")]
  jump: Option<TraceJump>,
  #[cfg(feature = "tracing")]
  start_time: std::time::Instant,
}

impl<'a> Printer<'a> {
  pub fn new(arena: &'a Arena, start_node: Option<PrintItemPath<'a>>, options: PrinterOptions) -> Printer<'a> {
    Printer {
      arena,
      possible_new_line_save_point: None,
      new_line_group_depth: 0,
      force_no_newlines_depth: 0,
      current_node: start_node,
      writer: Writer::new(
        arena,
        WriterOptions {
          indent_width: options.indent_width,
          #[cfg(feature = "tracing")]
          enable_tracing: options.enable_tracing,
        },
      ),
      resolved_conditions: BumpHashMap::with_hasher_in(Default::default(), arena.bump()),
      resolved_line_number_anchors: VecU32U32Map::with_capacity(thread_state::next_line_number_anchor_id()),
      resolved_line_numbers: VecU32U32Map::with_capacity(thread_state::next_line_number_id()),
      resolved_column_numbers: VecU32U32Map::with_capacity(thread_state::next_column_number_id()),
      resolved_is_start_of_lines: VecU32BoolMap::with_capacity(thread_state::next_is_start_of_line_id()),
      resolved_indent_levels: VecU32U8Map::with_capacity(thread_state::next_indent_level_id()),
      resolved_line_start_column_numbers: VecU32U32Map::with_capacity(thread_state::next_line_start_column_number_id()),
      resolved_line_start_indent_levels: VecU32U8Map::with_capacity(thread_state::next_line_start_indent_level_id()),
      look_ahead_save_points: LookAheadSavePoints::default(),
      infinite_reevaluation_protector: InfiniteReevaluationProtector::with_capacity(thread_state::next_condition_reevaluation_id()),
      stored_condition_save_points: BumpHashMap::with_hasher_in(Default::default(), arena.bump()),
      next_node_stack: NodeStack::default(),
      max_width: options.max_width,
      skip_moving_next: false,
      resolving_save_point: None,
      measurement: None,
      requested_measurement: None,
      measurement_results: BumpHashMap::with_hasher_in(Default::default(), arena.bump()),
      #[cfg(feature = "tracing")]
      traces: if options.enable_tracing { Some(Vec::new()) } else { None },
      #[cfg(feature = "tracing")]
      jump: None,
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

      if self.measurement.is_some() && matches!(current_node.item, PrintItem::String(_)) && self.is_measured_text_too_wide() {
        self.finish_measurement(false);
      }

      if self.skip_moving_next {
        self.skip_moving_next = false;
      } else if let Some(current_node) = self.current_node {
        self.current_node = current_node.get_next();
      }

      while self.current_node.is_none() && !self.next_node_stack.is_empty() {
        self.current_node = self.next_node_stack.pop();
      }

      if self.current_node.is_none() && self.measurement.is_some() {
        // the end of the print items is the end of the line
        self.finish_measurement(true);
        self.skip_moving_next = false;
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
        jump: self.jump.take(),
      });
    }
  }

  /// Records why the printer is going back to an earlier point when tracing.
  #[cfg(feature = "tracing")]
  fn trace_jump(&mut self, get_jump: impl FnOnce(&Self) -> TraceJump) {
    if self.traces.is_some() {
      self.jump = Some(get_jump(self));
    }
  }

  #[cfg(feature = "tracing")]
  fn get_trace_info_jump(&self, info: &Info) -> TraceInfoJump {
    let value = match info {
      Info::LineNumber(info) => self.resolved_line_numbers.get(info.unique_id()).map(TraceInfoValue::Number),
      Info::ColumnNumber(info) => self.resolved_column_numbers.get(info.unique_id()).map(TraceInfoValue::Number),
      Info::IsStartOfLine(info) => self.resolved_is_start_of_lines.get(info.unique_id()).map(TraceInfoValue::Bool),
      Info::IndentLevel(info) => self
        .resolved_indent_levels
        .get(info.unique_id())
        .map(|value| TraceInfoValue::Number(value as u32)),
      Info::LineStartColumnNumber(info) => self.resolved_line_start_column_numbers.get(info.unique_id()).map(TraceInfoValue::Number),
      Info::LineStartIndentLevel(info) => self
        .resolved_line_start_indent_levels
        .get(info.unique_id())
        .map(|value| TraceInfoValue::Number(value as u32)),
    };
    TraceInfoJump {
      info: TraceInfo::from(info),
      value: value.expect("the info was resolved before going back to the save point"),
    }
  }

  #[inline]
  pub fn get_writer_info(&self) -> WriterInfo {
    self.writer.writer_info()
  }

  pub fn resolved_line_number(&mut self, line_number: LineNumber) -> Option<u32> {
    let value = self.resolved_line_numbers.get(line_number.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(LookAheadKey::LineNumber(line_number.unique_id()), line_number.name());
    }
    value
  }

  pub fn resolved_column_number(&mut self, column_number: ColumnNumber) -> Option<u32> {
    let value = self.resolved_column_numbers.get(column_number.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(LookAheadKey::ColumnNumber(column_number.unique_id()), column_number.name());
    }
    value
  }

  pub fn resolved_is_start_of_line(&mut self, is_start_of_line: IsStartOfLine) -> Option<bool> {
    let value = self.resolved_is_start_of_lines.get(is_start_of_line.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(LookAheadKey::IsStartOfLine(is_start_of_line.unique_id()), is_start_of_line.name());
    }
    value
  }

  pub fn resolved_indent_level(&mut self, indent_level: IndentLevel) -> Option<u8> {
    let value = self.resolved_indent_levels.get(indent_level.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(LookAheadKey::IndentLevel(indent_level.unique_id()), indent_level.name());
    }
    value
  }

  pub fn resolved_line_start_column_number(&mut self, line_start_column_number: LineStartColumnNumber) -> Option<u32> {
    let value = self.resolved_line_start_column_numbers.get(line_start_column_number.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(
        LookAheadKey::LineStartColumnNumber(line_start_column_number.unique_id()),
        line_start_column_number.name(),
      );
    }
    value
  }

  pub fn resolved_line_start_indent_level(&mut self, line_start_indent_level: LineStartIndentLevel) -> Option<u8> {
    let value = self.resolved_line_start_indent_levels.get(line_start_indent_level.unique_id());
    if value.is_none() {
      self.add_look_ahead_save_point(
        LookAheadKey::LineStartIndentLevel(line_start_indent_level.unique_id()),
        line_start_indent_level.name(),
      );
    }
    value
  }

  pub fn clear_info(&mut self, info: Info) {
    self.record_info_for_measurement(&info);
    match info {
      Info::LineNumber(info) => self.resolved_line_numbers.remove(info.unique_id()),
      Info::ColumnNumber(info) => self.resolved_column_numbers.remove(info.unique_id()),
      Info::IsStartOfLine(info) => self.resolved_is_start_of_lines.remove(info.unique_id()),
      Info::IndentLevel(info) => self.resolved_indent_levels.remove(info.unique_id()),
      Info::LineStartColumnNumber(info) => self.resolved_line_start_column_numbers.remove(info.unique_id()),
      Info::LineStartIndentLevel(info) => self.resolved_line_start_indent_levels.remove(info.unique_id()),
    }
  }

  /// Moves an already resolved line number by `delta` lines.
  ///
  /// This keeps a resolution usable when whatever it refers to has shifted up or down the page
  /// without otherwise changing, leaving it correct relative to everything around it.
  pub fn shift_line_number(&mut self, info: LineNumber, delta: isize) {
    let id = info.unique_id();
    if let Some(value) = self.resolved_line_numbers.get(id) {
      self.record_for_measurement(|_| MeasurementUndo::LineNumber(id, Some(value)));
      self.resolved_line_numbers.insert(id, (value as isize + delta) as u32);
    }
  }

  /// Gets if the print items from the position fit on the rest of the line without newlines
  /// up to the line number, along with the items after it up to where the line would break.
  ///
  /// Returns `None` the first time at a position, then the printer measures and resolves the
  /// condition again.
  pub fn fits_on_line_until(&mut self, line_number: LineNumber, position: (u32, u32), stop_at_first_break: bool) -> Option<bool> {
    let request = MeasurementRequest {
      end_line_number_id: line_number.unique_id(),
      stop_at_first_break,
    };
    if let Some(measurement) = self.measurement.as_mut() {
      // everything is on one line while measuring, and when it all fits, so does this
      if !measurement.is_past_end {
        measurement.nested.push((request, position));
      }
      return Some(true);
    }
    match self.measurement_results.get(&request.end_line_number_id) {
      Some(result) if result.start == position && result.stop_at_first_break == stop_at_first_break => Some(result.fits),
      _ => {
        self.requested_measurement = Some(request);
        None
      }
    }
  }

  /// Gets if the printer is measuring whether print items fit on the line, in which
  /// case what it prints and resolves is undone afterwards.
  pub fn is_measuring(&self) -> bool {
    self.measurement.is_some()
  }

  /// Gets if the line could break before the current position.
  pub fn can_break_earlier_on_line(&self) -> bool {
    self.possible_new_line_save_point.is_some()
  }

  fn start_measurement(&mut self, request: MeasurementRequest, name: &'static str) {
    let save_point = self.create_save_point(name, self.current_node);
    self.measurement = Some(Measurement {
      request,
      save_point,
      start: (self.writer.line_number(), self.writer.column_number()),
      is_past_end: false,
      new_line_group_depth: self.new_line_group_depth,
      force_no_newlines_depth: self.force_no_newlines_depth,
      nested: Vec::new(),
      undo: Vec::new(),
      #[cfg(debug_assertions)]
      snapshot: self.measurement_snapshot(),
    });
    // measure the items up to the end on one line
    self.force_no_newlines_depth += 1;
  }

  fn finish_measurement(&mut self, fits: bool) {
    let measurement = self.measurement.take().unwrap();
    for undo in measurement.undo.into_iter().rev() {
      match undo {
        MeasurementUndo::LineNumber(id, value) => restore_u32(&mut self.resolved_line_numbers, id, value),
        MeasurementUndo::ColumnNumber(id, value) => restore_u32(&mut self.resolved_column_numbers, id, value),
        MeasurementUndo::LineStartColumnNumber(id, value) => restore_u32(&mut self.resolved_line_start_column_numbers, id, value),
        MeasurementUndo::LineNumberAnchor(id, value) => restore_u32(&mut self.resolved_line_number_anchors, id, value),
        MeasurementUndo::IndentLevel(id, value) => restore_u8(&mut self.resolved_indent_levels, id, value),
        MeasurementUndo::LineStartIndentLevel(id, value) => restore_u8(&mut self.resolved_line_start_indent_levels, id, value),
        MeasurementUndo::IsStartOfLine(id, value) => match value {
          Some(value) => self.resolved_is_start_of_lines.insert(id, value),
          None => self.resolved_is_start_of_lines.remove(id),
        },
        MeasurementUndo::Condition(id, value) => match value {
          Some(value) => {
            self.resolved_conditions.insert(id, value);
          }
          None => {
            self.resolved_conditions.remove(&id);
          }
        },
        MeasurementUndo::StoredConditionSavePoint(id, value) => match value {
          Some(value) => {
            self.stored_condition_save_points.insert(id, value);
          }
          None => {
            self.stored_condition_save_points.remove(&id);
          }
        },
      }
    }
    #[cfg(debug_assertions)]
    self.verify_measurement_restored(&measurement.snapshot);

    if fits {
      for (request, start) in measurement.nested {
        self.measurement_results.insert(
          request.end_line_number_id,
          MeasurementResult {
            start,
            stop_at_first_break: request.stop_at_first_break,
            fits: true,
          },
        );
      }
    }
    self.measurement_results.insert(
      measurement.request.end_line_number_id,
      MeasurementResult {
        start: measurement.start,
        stop_at_first_break: measurement.request.stop_at_first_break,
        fits,
      },
    );
    #[cfg(feature = "tracing")]
    self.trace_jump(|_| TraceJump::Measurement(TraceMeasurementJump { fits }));
    self.update_state_to_save_point(measurement.save_point, false);

    fn restore_u32(map: &mut VecU32U32Map, id: u32, value: Option<u32>) {
      match value {
        Some(value) => map.insert(id, value),
        None => map.remove(id),
      }
    }

    fn restore_u8(map: &mut VecU32U8Map, id: u32, value: Option<u8>) {
      match value {
        Some(value) => map.insert(id, value),
        None => map.remove(id),
      }
    }
  }

  /// Gets if the text written while measuring goes past the line width.
  ///
  /// Like when printing, only text that's allowed to cause a newline counts, which excludes
  /// text printed without newlines (ex. a trailing comment).
  fn is_measured_text_too_wide(&self) -> bool {
    let measurement = self.measurement.as_ref().unwrap();
    let measured_depth = measurement.force_no_newlines_depth + if measurement.is_past_end { 0 } else { 1 };
    self.force_no_newlines_depth == measured_depth && self.writer.column_number() > self.max_width
  }

  #[inline]
  fn record_for_measurement(&mut self, get_undo: impl FnOnce(&Self) -> MeasurementUndo<'a>) {
    if self.measurement.is_some() {
      let undo = get_undo(self);
      if let Some(measurement) = self.measurement.as_mut() {
        measurement.undo.push(undo);
      }
    }
  }

  fn record_info_for_measurement(&mut self, info: &Info) {
    self.record_for_measurement(|printer| match info {
      Info::LineNumber(info) => MeasurementUndo::LineNumber(info.unique_id(), printer.resolved_line_numbers.get(info.unique_id())),
      Info::ColumnNumber(info) => MeasurementUndo::ColumnNumber(info.unique_id(), printer.resolved_column_numbers.get(info.unique_id())),
      Info::IsStartOfLine(info) => MeasurementUndo::IsStartOfLine(info.unique_id(), printer.resolved_is_start_of_lines.get(info.unique_id())),
      Info::IndentLevel(info) => MeasurementUndo::IndentLevel(info.unique_id(), printer.resolved_indent_levels.get(info.unique_id())),
      Info::LineStartColumnNumber(info) => {
        MeasurementUndo::LineStartColumnNumber(info.unique_id(), printer.resolved_line_start_column_numbers.get(info.unique_id()))
      }
      Info::LineStartIndentLevel(info) => {
        MeasurementUndo::LineStartIndentLevel(info.unique_id(), printer.resolved_line_start_indent_levels.get(info.unique_id()))
      }
    });
  }

  #[cfg(debug_assertions)]
  fn measurement_snapshot(&self) -> MeasurementSnapshot {
    let mut conditions = self.resolved_conditions.iter().map(|(id, value)| (*id, *value)).collect::<Vec<_>>();
    conditions.sort();
    let mut stored_save_points = self
      .stored_condition_save_points
      .iter()
      .map(|(id, (_, save_point))| (*id, *save_point as *const SavePoint as usize))
      .collect::<Vec<_>>();
    stored_save_points.sort();
    MeasurementSnapshot {
      line_numbers: self.resolved_line_numbers.clone(),
      column_numbers: self.resolved_column_numbers.clone(),
      is_start_of_lines: self.resolved_is_start_of_lines.clone(),
      indent_levels: self.resolved_indent_levels.clone(),
      line_start_column_numbers: self.resolved_line_start_column_numbers.clone(),
      line_start_indent_levels: self.resolved_line_start_indent_levels.clone(),
      line_number_anchors: self.resolved_line_number_anchors.clone(),
      conditions,
      stored_save_points,
    }
  }

  #[cfg(debug_assertions)]
  fn verify_measurement_restored(&self, snapshot: &MeasurementSnapshot) {
    if self.measurement_snapshot() != *snapshot {
      panic!("Debug panic! Measuring whether print items fit on the line didn't restore what it resolved.");
    }
  }

  pub fn resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
    if !self.resolved_conditions.contains_key(&condition_reference.id) {
      self.add_look_ahead_save_point(LookAheadKey::Condition(condition_reference.id), condition_reference.name());
    }

    let result = self.resolved_conditions.get(&condition_reference.id)?;
    result.map(|x| x.to_owned())
  }

  /// Remembers to go back to the condition being resolved once what it looked ahead at is resolved.
  fn add_look_ahead_save_point(&mut self, key: LookAheadKey, name: &'static str) {
    if !self.look_ahead_save_points.contains(key) {
      let save_point = self.get_save_point_for_restoring_condition(name);
      self.look_ahead_save_points.insert(key, save_point, self.arena);
    }
  }

  pub fn is_forcing_no_newlines(&self) -> bool {
    self.force_no_newlines_depth > 0
  }

  #[inline]
  fn handle_print_node(&mut self, print_node: &PrintNode<'a>) {
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

  fn create_save_point(&self, _name: &'static str, next_node: Option<PrintItemPath<'a>>) -> &'a SavePoint<'a> {
    self.arena.alloc_save_point(SavePoint {
      #[cfg(debug_assertions)]
      name: _name,
      possible_new_line_save_point: self.possible_new_line_save_point,
      new_line_group_depth: self.new_line_group_depth,
      force_no_newlines_depth: self.force_no_newlines_depth,
      node: next_node,
      writer_state: self.writer.state(),
      look_ahead_save_points: self.look_ahead_save_points,
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
    if let Some(new_line_save_point) = &self.possible_new_line_save_point
      && self.new_line_group_depth > new_line_save_point.new_line_group_depth
    {
      return;
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
    self.look_ahead_save_points = save_point.look_ahead_save_points;
    self.next_node_stack = save_point.next_node_stack.clone();

    if is_for_new_line {
      self.write_new_line();
    }

    self.skip_moving_next = true;
  }

  #[inline]
  fn handle_signal(&mut self, signal: &Signal) {
    if let Some(measurement) = &self.measurement {
      match signal {
        Signal::NewLine | Signal::ExpectNewLine if !measurement.is_past_end => {
          // the items up to the end can't be on one line
          self.finish_measurement(false);
          return;
        }
        Signal::NewLine | Signal::ExpectNewLine => {
          self.finish_measurement(true);
          return;
        }
        Signal::PossibleNewLine | Signal::SpaceOrNewLine if measurement.is_past_end => {
          // the line breaks at the first possible newline after the end that's in the same or an
          // outer new line group, because the printer breaks earlier on the line before breaking
          // in a deeper one
          if measurement.request.stop_at_first_break || self.new_line_group_depth <= measurement.new_line_group_depth {
            self.finish_measurement(true);
          } else if matches!(signal, Signal::SpaceOrNewLine) {
            self.writer.space_if_not_trailing();
          }
          return;
        }
        _ => {}
      }
    }
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
            let optional_save_state = self.possible_new_line_save_point.take();
            if optional_save_state.is_none() {
              self.write_new_line();
            } else if let Some(save_state) = optional_save_state {
              if save_state.new_line_group_depth >= self.new_line_group_depth {
                self.write_new_line();
              } else {
                #[cfg(feature = "tracing")]
                self.trace_jump(|_| TraceJump::NewLine);
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
      Signal::StartResettingIndent => self.writer.start_resetting_indent(),
      Signal::FinishResettingIndent => self.writer.finish_resetting_indent(),
      Signal::StartAlign(columns) => self.writer.start_align(*columns),
      Signal::FinishAlign => self.writer.finish_align(),
      Signal::StartForceNoNewLines => self.force_no_newlines_depth += 1,
      Signal::FinishForceNoNewLines => self.force_no_newlines_depth -= 1,
      Signal::SpaceIfNotTrailing => self.writer.space_if_not_trailing(),
      Signal::KeepLineBeforeHere => self.possible_new_line_save_point = None,
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
              self.record_for_measurement(|_| MeasurementUndo::LineNumber(line_number_id, Some(value)));
              let new_value = ((value as isize) + difference) as u32;
              self.resolved_line_numbers.insert(line_number_id, new_value);
            }
          }
        }
        self.record_for_measurement(|printer| MeasurementUndo::LineNumberAnchor(id, printer.resolved_line_number_anchors.get(id)));
        self.resolved_line_number_anchors.insert(id, current_line_number);
      }
    }
  }

  #[inline]
  fn handle_targeted_info(&mut self, info: &Info) {
    if self.measurement.is_some() {
      self.record_info_for_measurement(info);
      self.resolve_info_while_measuring(info);
      return;
    }
    let save_point = match info {
      Info::LineNumber(line_number) => {
        let line_number_id = line_number.unique_id();
        self.resolved_line_numbers.insert(line_number_id, self.writer.line_number());
        self.look_ahead_save_points.remove(LookAheadKey::LineNumber(line_number_id), self.arena)
      }
      Info::ColumnNumber(column_number) => {
        let column_number_id = column_number.unique_id();
        self.resolved_column_numbers.insert(column_number_id, self.writer.column_number());
        self.look_ahead_save_points.remove(LookAheadKey::ColumnNumber(column_number_id), self.arena)
      }
      Info::IsStartOfLine(is_start_of_line) => {
        let is_start_of_line_id = is_start_of_line.unique_id();
        self.resolved_is_start_of_lines.insert(is_start_of_line_id, self.writer.is_start_of_line());
        self.look_ahead_save_points.remove(LookAheadKey::IsStartOfLine(is_start_of_line_id), self.arena)
      }
      Info::IndentLevel(indent_level) => {
        let indent_level_id = indent_level.unique_id();
        self.resolved_indent_levels.insert(indent_level_id, self.writer.indent_level());
        self.look_ahead_save_points.remove(LookAheadKey::IndentLevel(indent_level_id), self.arena)
      }
      Info::LineStartColumnNumber(line_start_column_number) => {
        let line_start_column_number_id = line_start_column_number.unique_id();
        self
          .resolved_line_start_column_numbers
          .insert(line_start_column_number_id, self.writer.line_start_column_number());
        self
          .look_ahead_save_points
          .remove(LookAheadKey::LineStartColumnNumber(line_start_column_number_id), self.arena)
      }
      Info::LineStartIndentLevel(line_start_indent_level) => {
        let line_start_indent_level_id = line_start_indent_level.unique_id();
        self
          .resolved_line_start_indent_levels
          .insert(line_start_indent_level_id, self.writer.line_start_indent_level());
        self
          .look_ahead_save_points
          .remove(LookAheadKey::LineStartIndentLevel(line_start_indent_level_id), self.arena)
      }
    };
    if let Some(save_point) = save_point {
      #[cfg(feature = "tracing")]
      self.trace_jump(|printer| TraceJump::Info(printer.get_trace_info_jump(info)));
      self.update_state_to_save_point(save_point, false);
    }
  }

  /// Resolves the info without going back to the conditions that looked ahead at it,
  /// since what's resolved while measuring is undone.
  fn resolve_info_while_measuring(&mut self, info: &Info) {
    match info {
      Info::LineNumber(info) => {
        self.resolved_line_numbers.insert(info.unique_id(), self.writer.line_number());
        let measurement = self.measurement.as_mut().unwrap();
        if info.unique_id() == measurement.request.end_line_number_id && !measurement.is_past_end {
          // measure the rest of the line normally
          measurement.is_past_end = true;
          self.force_no_newlines_depth -= 1;
        }
      }
      Info::ColumnNumber(info) => self.resolved_column_numbers.insert(info.unique_id(), self.writer.column_number()),
      Info::IsStartOfLine(info) => self.resolved_is_start_of_lines.insert(info.unique_id(), self.writer.is_start_of_line()),
      Info::IndentLevel(info) => self.resolved_indent_levels.insert(info.unique_id(), self.writer.indent_level()),
      Info::LineStartColumnNumber(info) => self
        .resolved_line_start_column_numbers
        .insert(info.unique_id(), self.writer.line_start_column_number()),
      Info::LineStartIndentLevel(info) => self
        .resolved_line_start_indent_levels
        .insert(info.unique_id(), self.writer.line_start_indent_level()),
    }
  }

  #[inline]
  fn handle_condition_reevaluation(&mut self, condition_reevaluation: &ConditionReevaluation) {
    if self.measurement.is_some() {
      // the condition was resolved before the measurement, so it's re-evaluated after
      return;
    }
    let condition_id = condition_reevaluation.condition_id;
    if let Some((condition, save_point)) = self.stored_condition_save_points.get(&condition_id).cloned()
      && let Some(past_condition_value) = self.resolved_conditions.get(&condition_id).and_then(|x| x.to_owned())
    {
      self.resolving_save_point.replace(save_point);
      let mut context = ConditionResolverContext::new(self, save_point.writer_state.writer_info(self.writer.indent_width()));
      let latest_condition_value = condition.resolve(&mut context);
      self.resolving_save_point.take();
      // the position re-evaluated at was measured when the condition was resolved
      self.requested_measurement = None;

      // Do not re-evaluate the condition if it's flipped back and forth a decent number of times.
      // If it hits the max number of times it can flip then an error will be logged.
      let should_reevaluate = self.infinite_reevaluation_protector.should_reevaluate(
        condition_reevaluation.condition_reevaluation_id,
        condition.name(),
        latest_condition_value,
        past_condition_value,
      );
      if should_reevaluate {
        if let Some(latest_condition_value) = latest_condition_value {
          if latest_condition_value != past_condition_value {
            #[cfg(feature = "tracing")]
            self.trace_jump(|_| {
              TraceJump::ConditionReevaluation(TraceConditionJump {
                condition_id,
                name: condition.name().to_string(),
                value: Some(latest_condition_value),
              })
            });
            self.update_state_to_save_point(save_point, false);
          }
        } else {
          self.resolved_conditions.remove(&condition_id);
        }
      }
    }
  }

  #[inline]
  fn handle_condition(&mut self, condition: &'a Condition<'a>, next_node: &Option<PrintItemPath<'a>>) {
    let condition_id = condition.unique_id();

    if condition.store_save_point {
      let save_point = self.get_save_point_for_restoring_condition(condition.name());
      self.record_for_measurement(|printer| {
        MeasurementUndo::StoredConditionSavePoint(condition_id, printer.stored_condition_save_points.get(&condition_id).copied())
      });
      self.stored_condition_save_points.insert(condition.unique_id(), (condition, save_point));
    }

    let condition_value = condition.resolve(&mut ConditionResolverContext::new(self, self.get_writer_info()));
    if condition.is_stored {
      self.record_for_measurement(|printer| MeasurementUndo::Condition(condition_id, printer.resolved_conditions.get(&condition_id).copied()));
      self.resolved_conditions.insert(condition_id, condition_value);
    }

    if let Some(request) = self.requested_measurement.take() {
      self.start_measurement(request, condition.name());
    }

    if condition_value.is_some()
      && self.measurement.is_none()
      && let Some(save_point) = self.look_ahead_save_points.remove(LookAheadKey::Condition(condition_id), self.arena)
    {
      #[cfg(feature = "tracing")]
      self.trace_jump(|_| {
        TraceJump::Condition(TraceConditionJump {
          condition_id,
          name: condition.name().to_string(),
          value: condition_value,
        })
      });
      self.update_state_to_save_point(save_point, false);
      return;
    }

    if condition_value.is_some() && condition_value.unwrap() {
      if let Some(true_path) = condition.true_path {
        self.current_node = Some(true_path);
        if let Some(path) = next_node {
          self.next_node_stack.push(path, self.arena);
        }
        self.skip_moving_next = true;
      }
    } else if let Some(false_path) = condition.false_path {
      self.current_node = Some(false_path);
      if let Some(path) = next_node {
        self.next_node_stack.push(path, self.arena);
      }
      self.skip_moving_next = true;
    }
  }

  #[inline]
  fn handle_rc_path(&mut self, print_item_path: &PrintItemPath<'a>, next_node: &Option<PrintItemPath<'a>>) {
    if let Some(path) = next_node {
      self.next_node_stack.push(path, self.arena);
    }
    self.current_node = Some(print_item_path);
    self.skip_moving_next = true;
  }

  #[inline]
  fn handle_string(&mut self, text: &'a StringContainer) {
    #[cfg(debug_assertions)]
    self.validate_string(text.text);

    if self.measurement.is_none() && self.possible_new_line_save_point.is_some() && self.is_above_max_width(text.char_count) && self.allow_new_lines() {
      let save_point = self.possible_new_line_save_point.take();
      #[cfg(feature = "tracing")]
      self.trace_jump(|_| TraceJump::NewLine);
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
      panic!(
        "Debug panic! Found a newline in the string. Before sending the string to the printer it needs to be broken up and the newline sent as a PrintItem::NewLine. {0}",
        text
      );
    }
  }

  #[cfg(debug_assertions)]
  fn verify_no_look_ahead_save_points(&self) {
    // The look ahead save points should be empty when printing is finished. If it's not
    // then that indicates that the generator tried to resolve a condition or info that was
    // never added to the print items. In this scenario, the look ahead hash maps will
    // be cloned when creating a save point and contain items that don't need to exist
    // in them thus having an unnecessary performance impact.
    if let Some(save_point) = self.look_ahead_save_points.save_points().next() {
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
    if self.writer.is_resetting_indent() {
      panic!("Debug panic! The writer was still resetting the indent after printing.");
    }
    if self.writer.is_aligning() {
      panic!("Debug panic! The writer was still aligning after printing.");
    }
  }
}

/// A measurement of whether print items fit on the rest of the line.
struct Measurement<'a> {
  request: MeasurementRequest,
  /// Where to go back to once the measurement is done.
  save_point: &'a SavePoint<'a>,
  start: (u32, u32),
  /// Whether the end was printed, after which the rest of the line is measured normally.
  is_past_end: bool,
  /// The new line group depth when the measurement started.
  new_line_group_depth: u16,
  /// The force no newlines depth when the measurement started.
  force_no_newlines_depth: u8,
  /// The measurements requested within this one, which fit when this one does.
  nested: Vec<(MeasurementRequest, (u32, u32))>,
  /// The values from before the measurement changed them.
  undo: Vec<MeasurementUndo<'a>>,
  #[cfg(debug_assertions)]
  snapshot: MeasurementSnapshot,
}

#[derive(Clone, Copy)]
struct MeasurementRequest {
  end_line_number_id: u32,
  /// Whether the line ends at the first possible newline after the end, regardless of its new line group.
  stop_at_first_break: bool,
}

#[derive(Clone, Copy)]
struct MeasurementResult {
  start: (u32, u32),
  stop_at_first_break: bool,
  fits: bool,
}

enum MeasurementUndo<'a> {
  LineNumber(u32, Option<u32>),
  ColumnNumber(u32, Option<u32>),
  IsStartOfLine(u32, Option<bool>),
  IndentLevel(u32, Option<u8>),
  LineStartColumnNumber(u32, Option<u32>),
  LineStartIndentLevel(u32, Option<u8>),
  LineNumberAnchor(u32, Option<u32>),
  Condition(u32, Option<Option<bool>>),
  StoredConditionSavePoint(u32, Option<(&'a Condition<'a>, &'a SavePoint<'a>)>),
}

/// The values a measurement could change, to check that it restored them.
#[cfg(debug_assertions)]
#[derive(PartialEq)]
struct MeasurementSnapshot {
  line_numbers: VecU32U32Map,
  column_numbers: VecU32U32Map,
  is_start_of_lines: VecU32BoolMap,
  indent_levels: VecU32U8Map,
  line_start_column_numbers: VecU32U32Map,
  line_start_indent_levels: VecU32U8Map,
  line_number_anchors: VecU32U32Map,
  conditions: Vec<(u32, Option<bool>)>,
  stored_save_points: Vec<(u32, usize)>,
}
