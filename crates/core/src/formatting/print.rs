use super::*;

/// Options for printing the print items.
pub struct PrintOptions {
  /// The width the printer will attempt to keep the line under.
  pub max_width: u32,
  /// The number of columns to count when indenting or using a tab.
  pub indent_width: u8,
  /// Whether to use tabs for indenting.
  pub use_tabs: bool,
  /// The newline character to use when doing a new line.
  pub new_line_text: &'static str,
}

impl PrintOptions {
  pub(super) fn to_printer_options(&self) -> PrinterOptions {
    PrinterOptions {
      indent_width: self.indent_width,
      max_width: self.max_width,
      #[cfg(feature = "tracing")]
      enable_tracing: false,
    }
  }
}

/// Creates the print items with the provided closure and prints them out as a string.
///
/// Use [`Arena::new`] and [`print`] instead when the print items need to borrow data
/// that's created outside the closure, such as the text of the file being formatted,
/// or use [`ArenaPool::create_arena`] to reuse the memory of the arena between formats.
pub fn format(get_print_items: impl for<'a> FnOnce(&'a Arena) -> PrintItems<'a>, options: PrintOptions) -> String {
  let arena = Arena::new();
  let print_items = get_print_items(&arena);
  print(print_items, options)
}

/// Prints out the print items using the provided options.
pub fn print(print_items: PrintItems, options: PrintOptions) -> String {
  match Printer::new(print_items.arena(), print_items.first_node, options.to_printer_options()).print() {
    Some(write_items) => WriteItemsPrinter::from(&options).print(write_items),
    None => String::new(),
  }
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracingResult {
  pub traces: Vec<Trace>,
  pub writer_nodes: Vec<TraceWriterNode>,
  pub print_nodes: Vec<TracePrintNode>,
}

/// Gets trace information for analysis purposes.
#[cfg(feature = "tracing")]
pub fn trace_printing(print_items: PrintItems, options: PrintOptions) -> TracingResult {
  use std::iter;

  {
    let tracing_result = Printer::new(print_items.arena(), print_items.first_node, {
      let mut printer_options = options.to_printer_options();
      printer_options.enable_tracing = true;
      printer_options
    })
    .print_for_tracing();
    let writer_items_printer = WriteItemsPrinter::from(&options);

    TracingResult {
      traces: tracing_result.traces,
      writer_nodes: tracing_result
        .writer_nodes
        .into_iter()
        .map(|node| {
          let text = writer_items_printer.print(iter::once(node.item));
          TraceWriterNode {
            writer_node_id: node.graph_node_id,
            previous_node_id: node.previous.map(|n| n.graph_node_id),
            text,
          }
        })
        .collect(),
      print_nodes: super::get_trace_print_nodes(print_items.first_node),
    }
  }
}

#[cfg(test)]
mod test {
  use crate::formatting::LineNumber;

  use super::super::IntoPrintItems;
  use super::super::PrintItems;
  use super::PrintOptions;
  use super::format;

  #[test]
  fn test_format_in_format() {
    assert_eq!(
      format(
        |arena| {
          let mut items = PrintItems::new(arena);
          assert_eq!(LineNumber::new("").unique_id(), 0);
          assert_eq!(LineNumber::new("").unique_id(), 1);
          assert_eq!(LineNumber::new("").unique_id(), 2);
          items.push_str_runtime_width_computed("test");
          items.push_string(format(
            |arena| {
              // It's important that these start incrementing from
              // 0 when formatting within a format because these
              // are stored as resolved within the printer using
              // a vector and the id is the index
              assert_eq!(LineNumber::new("").unique_id(), 0);
              assert_eq!(LineNumber::new("").unique_id(), 1);
              "test".into_print_items(arena)
            },
            get_print_options(),
          ));
          // now ensure it goes back to where it left off
          assert_eq!(LineNumber::new("").unique_id(), 3);
          items
        },
        get_print_options(),
      ),
      "testtest"
    );
  }

  #[test]
  fn test_reset_indent() {
    use super::super::Signal;

    assert_eq!(
      format(
        |arena| {
          let mut items = PrintItems::new(arena);
          items.push_signal(Signal::StartIndent);
          items.push_str_runtime_width_computed("a");
          items.push_signal(Signal::NewLine);
          items.push_signal(Signal::StartIgnoringIndent);
          items.push_str_runtime_width_computed("    b");
          items.push_signal(Signal::FinishIgnoringIndent);
          // indent relative to the text written as is
          items.push_signal(Signal::StartResettingIndent);
          items.push_signal(Signal::StartIndent);
          items.push_signal(Signal::StartIndent);
          items.push_signal(Signal::StartIndent);
          items.push_signal(Signal::NewLine);
          items.push_str_runtime_width_computed("c");
          items.push_signal(Signal::FinishIndent);
          items.push_signal(Signal::FinishIndent);
          items.push_signal(Signal::FinishIndent);
          items.push_signal(Signal::FinishResettingIndent);
          items.push_signal(Signal::NewLine);
          items.push_str_runtime_width_computed("d");
          items.push_signal(Signal::FinishIndent);
          items
        },
        get_print_options(),
      ),
      "  a\n    b\n      c\n  d"
    );
  }

  fn get_print_options() -> PrintOptions {
    PrintOptions {
      max_width: 40,
      indent_width: 2,
      use_tabs: false,
      new_line_text: "\n",
    }
  }
}
