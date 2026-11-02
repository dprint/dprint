use std::cell::Cell;
use std::cell::RefCell;

use super::super::Arena;
use super::super::actions;
use super::super::print_items::*;

/// The number of times a region's infos are cleared before the region is assumed to break the
/// way it did when it started at the same column before, which only happens when it's nested in
/// regions that keep moving it.
const MAX_CLEARS_BEFORE_ASSUMING: u32 = 8;

/// Infos resolved within a region of print items, whose resolved values are only
/// correct for where the region started when they were resolved.
///
/// The printer keeps resolved infos when it goes back to an earlier point, so when the
/// text before a region changes, the infos within it are stale. When the printer reaches
/// the start of the region at a different position than before, this keeps them correct:
///
/// - When the region starts on a different line, but at the same column, it's assumed to
///   break the same way, so its line numbers are shifted by the number of lines it moved.
///   Clearing them would be correct too, but each one would have to be worked out again, along
///   with every one in the regions nested within it, so a region nested n deep would be printed
///   2^n times.
/// - When it starts at a column it started at before, its line numbers are set to what they
///   were there, shifted by the lines it moved, when it fit on one line there or after its
///   infos were cleared many times. A region nested in others that each go back and forth
///   between a few positions would otherwise be worked out again every time, so it would be
///   printed exponentially many times.
/// - Otherwise, it may break differently, so the infos are cleared to be worked out again.
///
/// Assuming the region fits on one line is corrected by the line breaking within it when it
/// doesn't, but assuming it's multi-line makes the conditions using the infos keep it multi-line.
/// So that's only assumed for regions that are worked out again so many times that it matters,
/// and not when its width on one line (which is remembered from when it was on one line) fits
/// from where it starts, in which case it's assumed to fit on one line instead.
///
/// Note: A region at the same column with a different indent level may break differently,
/// but clearing in that case changes how existing code is formatted, so it isn't done.
#[derive(Clone, Copy)]
pub struct InfoRegion<'a> {
  infos: &'a RefCell<bumpalo::collections::Vec<'a, Info>>,
  last_start: &'a Cell<Option<RegionStart>>,
  /// The line numbers resolved when the region started at each column it left.
  previous_starts: &'a RefCell<bumpalo::collections::Vec<'a, PreviousStart<'a>>>,
  /// The number of times the infos were cleared.
  clear_count: &'a Cell<u32>,
  /// The width of the region when it was on one line, which is the same wherever it starts.
  single_line_width: &'a Cell<Option<u32>>,
}

struct PreviousStart<'a> {
  start: RegionStart,
  /// The resolved value of each info that's a line number, by its index in the infos.
  line_numbers: bumpalo::collections::Vec<'a, Option<u32>>,
}

impl PreviousStart<'_> {
  /// Gets if the region's line numbers were all on the line it started on.
  fn was_single_line(&self) -> bool {
    self.line_numbers.iter().all(|value| *value == Some(self.start.line_number))
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RegionStart {
  line_number: u32,
  column_number: u32,
}

impl<'a> InfoRegion<'a> {
  pub fn new(arena: &'a Arena) -> Self {
    Self {
      infos: arena.bump().alloc(RefCell::new(bumpalo::collections::Vec::new_in(arena.bump()))),
      last_start: arena.bump().alloc(Cell::new(None)),
      previous_starts: arena.bump().alloc(RefCell::new(bumpalo::collections::Vec::new_in(arena.bump()))),
      clear_count: arena.bump().alloc(Cell::new(0)),
      single_line_width: arena.bump().alloc(Cell::new(None)),
    }
  }

  /// Adds an info that's resolved within the region.
  ///
  /// This can be done after generating the region's start, but must be done before printing.
  pub fn add(&self, info: impl Into<Info>) {
    self.infos.borrow_mut().push(info.into());
  }

  /// Generates the items for the start of the region, which keep the infos correct
  /// when the region starts at a different position than before.
  pub fn gen_start(&self, arena: &'a Arena) -> PrintItems<'a> {
    let infos = self.infos;
    let last_start = self.last_start;
    let previous_starts = self.previous_starts;
    let clear_count = self.clear_count;
    let single_line_width = self.single_line_width;
    actions::action(arena, "infoRegionStart", move |context| {
      let start = RegionStart {
        line_number: context.writer_info.line_number,
        column_number: context.writer_info.column_number,
      };
      if let Some(last_start) = last_start.get()
        && last_start != start
      {
        let infos = infos.borrow();
        if last_start.column_number == start.column_number {
          // only the line changed, which only changes the line numbers
          let delta = start.line_number as isize - last_start.line_number as isize;
          for info in infos.iter() {
            if let Info::LineNumber(line_number) = info {
              context.shift_line_number(*line_number, delta);
            }
          }
        } else {
          let mut previous_starts = previous_starts.borrow_mut();
          remember_line_numbers(&mut previous_starts, last_start, &infos, context, arena);
          let is_repeatedly_cleared = clear_count.get() >= MAX_CLEARS_BEFORE_ASSUMING;
          let fits_on_one_line = single_line_width.get().is_some_and(|width| start.column_number + width <= context.max_width());
          let previous = previous_starts
            .iter()
            .find(|previous| previous.start.column_number == start.column_number)
            .filter(|previous| previous.was_single_line() || is_repeatedly_cleared);
          match previous {
            Some(previous) if !previous.was_single_line() && fits_on_one_line => {
              // it was multi-line at this column, but it fits on one line, so assume that instead,
              // which the line breaking within it corrects when what's after it doesn't fit
              for info in infos.iter() {
                match info {
                  Info::LineNumber(line_number) => context.set_line_number(*line_number, start.line_number),
                  _ => context.clear_info(*info),
                }
              }
            }
            Some(previous) => {
              // assume it breaks the way it did when it started at this column before
              let delta = start.line_number as isize - previous.start.line_number as isize;
              for (info, value) in infos.iter().zip(previous.line_numbers.iter()) {
                match (info, value) {
                  (Info::LineNumber(line_number), Some(value)) => {
                    context.set_line_number(*line_number, (*value as isize + delta) as u32);
                  }
                  _ => context.clear_info(*info),
                }
              }
            }
            None => {
              clear_count.set(clear_count.get() + 1);
              for info in infos.iter() {
                context.clear_info(*info);
              }
            }
          }
        }
      }
      last_start.set(Some(start));
    })
  }

  /// Generates the items for the end of the region, which remember its width when it's on one line.
  pub fn gen_end(&self, arena: &'a Arena) -> PrintItems<'a> {
    let last_start = self.last_start;
    let single_line_width = self.single_line_width;
    actions::action(arena, "infoRegionEnd", move |context| {
      if let Some(start) = last_start.get()
        && context.writer_info.line_number == start.line_number
      {
        single_line_width.set(Some(context.writer_info.column_number - start.column_number));
      }
    })
  }
}

/// Remembers the line numbers that were resolved when the region started at the start it's leaving.
///
/// When it fit on one line at that column before, that's kept instead, because assuming it fits is
/// corrected when it doesn't, while assuming it's multi-line isn't.
fn remember_line_numbers<'a>(
  previous_starts: &mut bumpalo::collections::Vec<'a, PreviousStart<'a>>,
  start: RegionStart,
  infos: &[Info],
  context: &ConditionResolverContext,
  arena: &'a Arena,
) {
  let line_numbers = infos.iter().map(|info| match info {
    Info::LineNumber(line_number) => context.peek_line_number(*line_number),
    _ => None,
  });
  match previous_starts.iter_mut().find(|previous| previous.start.column_number == start.column_number) {
    Some(previous) => {
      let is_single_line = line_numbers.clone().all(|value| value == Some(start.line_number));
      if previous.was_single_line() && !is_single_line {
        return;
      }
      previous.start = start;
      previous.line_numbers.clear();
      previous.line_numbers.extend(line_numbers);
    }
    None => {
      let mut values = bumpalo::collections::Vec::with_capacity_in(infos.len(), arena.bump());
      values.extend(line_numbers);
      previous_starts.push(PreviousStart { start, line_numbers: values });
    }
  }
}

#[cfg(test)]
mod test {
  use super::super::super::*;
  use super::*;

  #[test]
  fn shifts_line_numbers_when_region_moves_down() {
    // A condition before the region puts a newline before it when the region is multi-line,
    // which moves it down. Without shifting, a condition in the region would then compare
    // its new start line with the end line from before it moved.
    let text = format_for_test(|arena| {
      let region = InfoRegion::new(arena);
      let start_ln = LineNumber::new("start");
      let end_ln = LineNumber::new("end");
      region.add(end_ln);
      let is_multi_line = move |context: &mut ConditionResolverContext| {
        let start_ln = context.resolved_line_number(start_ln)?;
        let end_ln = context.resolved_line_number(end_ln)?;
        Some(end_ln > start_ln)
      };
      let mut items = PrintItems::new(arena);
      items.push_condition(conditions::if_true(
        "newLineIfMultiLine",
        arena.alloc_resolver(is_multi_line),
        Signal::NewLine.into_print_items(arena),
      ));
      items.extend(region.gen_start(arena));
      items.push_info(start_ln);
      items.push_condition(conditions::if_true_or(
        "multiOrSingle",
        arena.alloc_resolver(is_multi_line),
        "multi".into_print_items(arena),
        "single".into_print_items(arena),
      ));
      items.push_signal(Signal::NewLine);
      items.push_str_runtime_width_computed("b");
      items.push_info(end_ln);
      items
    });
    assert_eq!(text, "\nmulti\nb");
  }

  #[test]
  fn clears_infos_when_region_moves_across() {
    // A condition before the region puts spaces before it when the region ends at column 1,
    // which moves it across. Without clearing, a condition in the region would use the end
    // column from before it moved instead of working it out again.
    let text = format_for_test(|arena| {
      let region = InfoRegion::new(arena);
      let end_cn = ColumnNumber::new("end");
      region.add(end_cn);
      let mut items = PrintItems::new(arena);
      items.push_condition(conditions::if_true(
        "spacesIfEndAtColumnOne",
        arena.alloc_resolver(move |context| Some(context.resolved_column_number(end_cn)? == 1)),
        "  ".into_print_items(arena),
      ));
      items.extend(region.gen_start(arena));
      items.push_condition(conditions::if_true_or(
        "longOrShort",
        arena.alloc_resolver(move |context| Some(context.resolved_column_number(end_cn)? > 2)),
        "L".into_print_items(arena),
        "S".into_print_items(arena),
      ));
      items.push_info(end_cn);
      items
    });
    assert_eq!(text, "  L");
  }

  #[test]
  fn keeps_infos_when_region_does_not_move() {
    let text = format_for_test(|arena| {
      let region = InfoRegion::new(arena);
      let end_cn = ColumnNumber::new("end");
      region.add(end_cn);
      let mut items = PrintItems::new(arena);
      items.extend(region.gen_start(arena));
      items.push_condition(conditions::if_true_or(
        "longOrShort",
        arena.alloc_resolver(move |context| Some(context.resolved_column_number(end_cn)? > 0)),
        "L".into_print_items(arena),
        "S".into_print_items(arena),
      ));
      items.push_info(end_cn);
      items
    });
    assert_eq!(text, "L");
  }

  fn format_for_test(get_print_items: impl for<'a> FnOnce(&'a Arena) -> PrintItems<'a>) -> String {
    format(
      get_print_items,
      PrintOptions {
        max_width: 40,
        indent_width: 2,
        use_tabs: false,
        new_line_text: "\n",
      },
    )
  }
}
