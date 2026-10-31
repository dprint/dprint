use std::cell::Cell;
use std::cell::RefCell;

use super::super::Arena;
use super::super::actions;
use super::super::print_items::*;

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
/// - Otherwise, it may break differently, so the infos are cleared to be worked out again.
///
/// Note: A region at the same column with a different indent level may break differently,
/// but clearing in that case changes how existing code is formatted, so it isn't done.
#[derive(Clone, Copy)]
pub struct InfoRegion<'a> {
  infos: &'a RefCell<bumpalo::collections::Vec<'a, Info>>,
  last_start: &'a Cell<Option<RegionStart>>,
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
    actions::action(arena, "infoRegionStart", move |context| {
      let start = RegionStart {
        line_number: context.writer_info.line_number,
        column_number: context.writer_info.column_number,
      };
      if let Some(last_start) = last_start.get()
        && last_start != start
      {
        if last_start.column_number == start.column_number {
          // only the line changed, which only changes the line numbers
          let delta = start.line_number as isize - last_start.line_number as isize;
          for info in infos.borrow().iter() {
            if let Info::LineNumber(line_number) = info {
              context.shift_line_number(*line_number, delta);
            }
          }
        } else {
          for info in infos.borrow().iter() {
            context.clear_info(*info);
          }
        }
      }
      last_start.set(Some(start));
    })
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
