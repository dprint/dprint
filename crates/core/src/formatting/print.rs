use bumpalo::Bump;
use std::cell::RefCell;

use super::utils::with_bump_allocator;
use super::utils::with_bump_allocator_mut;
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

/// Function to create the provided print items and print them out as a string.
///
/// Note: It is unsafe to use the print items created within `get_print_items`
/// outside of the closure, since they are created with a thread local allocator
/// that is reset once this function returns.
pub fn format(get_print_items: impl FnOnce() -> PrintItems, options: PrintOptions) -> String {
  increment_formatting_count();
  let print_items = get_print_items();

  with_bump_allocator_mut(|bump| {
    let result = print_with_allocator(bump, &print_items, &options);
    if decrement_formatting_count() {
      bump.reset();
    }
    result
  })
}

/// Prints out the print items using the provided options.
///
/// Note: This should only be used in rare scenarios. In most cases,
/// use only `dprint_core::formatting::format`.
pub fn print(print_items: PrintItems, options: PrintOptions) -> String {
  // This shouldn't be called without calling `format` because it doesn't
  // reset the allocator.
  panic_if_not_formatting();

  with_bump_allocator(|bump| print_with_allocator(bump, &print_items, &options))
}

fn print_with_allocator(bump: &Bump, print_items: &PrintItems, options: &PrintOptions) -> String {
  let write_items = Printer::new(bump, print_items.first_node, options.to_printer_options()).print();
  WriteItemsPrinter::from(options).print(write_items)
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
pub fn trace_printing(get_print_items: impl FnOnce() -> PrintItems, options: PrintOptions) -> TracingResult {
  use std::iter;

  increment_formatting_count();
  let print_items = get_print_items();

  let result = with_bump_allocator_mut(|bump| {
    let tracing_result = Printer::new(bump, print_items.first_node.clone(), {
      let mut printer_options = options.to_printer_options();
      printer_options.enable_tracing = true;
      printer_options
    })
    .print_for_tracing();
    let writer_items_printer = WriteItemsPrinter::from(&options);

    let result = TracingResult {
      traces: tracing_result.traces,
      writer_nodes: tracing_result
        .writer_nodes
        .into_iter()
        .map(|node| {
          let text = writer_items_printer.print(iter::once(*node.borrow_item()));
          TraceWriterNode {
            writer_node_id: node.graph_node_id,
            previous_node_id: node.borrow_previous().map(|n| n.graph_node_id),
            text,
          }
        })
        .collect(),
      print_nodes: super::get_trace_print_nodes(print_items.first_node.clone()),
    };

    if decrement_formatting_count() {
      bump.reset();
    }
    result
  });
  result
}

thread_local! {
    static FORMATTING_COUNT: RefCell<u32> = RefCell::new(0);
}

fn increment_formatting_count() {
  FORMATTING_COUNT.with(|formatting_count_cell| {
    let mut formatting_count = formatting_count_cell.borrow_mut();
    *formatting_count += 1;
  })
}

fn decrement_formatting_count() -> bool {
  FORMATTING_COUNT.with(|formatting_count_cell| {
    let mut formatting_count = formatting_count_cell.borrow_mut();
    *formatting_count -= 1;
    *formatting_count == 0
  })
}

fn panic_if_not_formatting() {
  FORMATTING_COUNT.with(|formatting_count_cell| {
    if *formatting_count_cell.borrow() == 0 {
      panic!("dprint_core::formatting::print cannot be called except within the provided closure to dprint_core::formatting::format");
    }
  })
}
