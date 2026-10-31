# Overview

This document describes how the `formatting` module of the `dprint-core` crate works. Using it is not required for creating a plugin, but it's what the official Rust plugins use.

1. Source code is parsed to an AST (recommended, but not required).
2. AST is traversed and IR is generated.
3. IR is printed by printer.

## IR Generation

The intermediate representation (IR) describes how the nodes should be formatted. It consists of...

1. Texts
1. Infos
1. Conditions
1. Signals
1. Anchors

These are referred to as "print items" in the code and are collected in a `PrintItems` struct.

### Texts

Strings that the printer should print. For example `"async"`. There are a few ways to add them:

- `items.push_sc(sc!("async"))` - Preferred for text known at compile time. The `sc!` macro from the [`dprint-core-macros`](https://crates.io/crates/dprint-core-macros) crate computes the width at compile time (ASCII only).
- `items.push_str_runtime_width_computed("async")` - A `&'static str` whose width is computed at runtime.
- `items.push_str(text)` - A borrowed string that gets copied into the printer's allocator.
- `items.push_string(text)` - An owned `String`.
- `items.push_space()` - A single space.

### Infos

These objects are invisible in the output. They may be placed into the IR and when resolved by the printer, report the following information about where the info ended up at:

- `LineNumber`
- `ColumnNumber`
- `IsStartOfLine`
- `IndentLevel`
- `LineStartIndentLevel`
- `LineStartColumnNumber`

There's also `LineAndColumn`, which resolves to both the line and column number (use `items.push_line_and_column(...)`).

Each info is created with a name for debugging purposes (ex. `LineNumber::new("start")`) and its value is read in a condition resolver (ex. `context.resolved_line_number(start_ln)`).

### Conditions

Conditions have three main properties:

- Optional true path - Print items to use when the condition is resolved as _true_.
- Optional false path - Print items to use when the condition is resolved as _false_.
- Condition resolver - Function or condition that the printer uses to resolve the condition as _true_ or _false_.

#### Condition Resolver

A condition resolver is a `ConditionResolver` (`Rc<dyn Fn(&mut ConditionResolverContext) -> Option<bool>>`). Returning `None` means the condition can't be resolved yet, which is treated as _false_.

Conditions are usually resolved by looking at the value of a resolved info, another condition (see `Condition::create_reference()`), the current writer position (`context.writer_info`), or based on the original AST node.

The infos & conditions that are inspected may appear before or even after the condition. When an info that wasn't resolved yet is later reached, the printer goes back and reprints from the first condition that tried to read it.

The `conditions`, `condition_resolvers`, and `condition_helpers` modules have common ones.

#### Condition Re-evaluations

Condition re-evaluations can be added to the graph to force a condition to be re-evaluated once the printer reaches that point. If the condition has changed then the printer will jump back to that condition.

```rs
let mut condition: Condition = ...;
let condition_reevaluation = condition.create_reevaluation();

let mut items = PrintItems::new(arena);
items.push_condition(condition);
// ...more items added here...

// forces the re-evaluation of the condition this re-evaluation
// was created from at this point
items.push_reevaluation(condition_reevaluation);
```

### Signals

This is an enum that signals information to the printer.

- `NewLine` - Signal that a new line should occur based on the printer settings.
- `Tab` - Signal that a tab should occur based on the printer settings (ex. if indent width is 4 it will increase the column width by 4 for each tab).
- `PossibleNewLine` - Signal that the current location could be a newline when exceeding the line width.
- `SpaceOrNewLine` - Signal that the current location should be a space, but could be a newline if exceeding the line width.
- `ExpectNewLine` - Expect the next character to be a newline. If it's not, force a newline. This is useful to use at the end of single line comments in JS, for example.
- `QueueStartIndent` - Queue a start indent to be set after the next written item.
- `StartIndent` - Signal the start of a section that should be indented.
- `FinishIndent` - Signal the end of a section that should be indented.
- `StartNewLineGroup` - Signal the start of a group of print items that have a lower precedence for being broken up with a newline for exceeding the line width.
- `FinishNewLineGroup` - Signal the end of a newline group.
- `SingleIndent` - Signal that a single indent should occur based on the printer settings (ex. prints a tab when using tabs).
- `StartIgnoringIndent` - Signal to the printer that it should stop using indentation.
- `FinishIgnoringIndent` - Signal to the printer that it should start using indentation again.
- `StartForceNoNewLines` - Signal to the printer that it shouldn't print any new lines.
- `FinishForceNoNewLines` - Signal to the printer that it should finish not printing any new lines.
- `SpaceIfNotTrailing` - Signal that a space should occur if not trailing.

The `ir_helpers` module has helpers for common uses of these (ex. `ir_helpers::with_indent(items)`).

### Anchors

Anchors provide a way to update infos in the future when an anchor's line number changes.

For example, given the following code...

```rs
let mut items = PrintItems::new(arena);
let end_ln = LineNumber::new("end");

// ...lots of code here...
items.push_anchor(LineNumberAnchor::new(end_ln));
items.extend(actions::action(arena, "outputEndLine", move |context| {
  eprintln!("{:?}", context.resolved_line_number(end_ln));
}));
// ...lots of code here...
items.push_signal(Signal::NewLine);
items.push_info(end_ln);
```

...the anchor at the start of the code will update the `end_ln` line number based on how the anchor's line number has changed since it was last evaluated. This can be useful when evaluating conditions that "look into the future".

## Printer

The printer takes the IR and outputs the final code. Its main responsibilities are:

1. Resolving infos and conditions in the IR.
2. Printing out the text with the correct indentation and newline kind.
3. Seeing where lines exceed the maximum line width and breaking up the line as specified in the IR.

#### Rules

The printer never checks the contents of the provided strings—it only looks at the width of the strings. For that reason there are certain rules:

1. Never use a tab in a string. Instead, use `Signal::Tab` (see _Signals_ above). Tabs increase the column width based on the indent width and need to be treated differently.
2. Never use a newline in a string. Instead use `Signal::NewLine`.

Strings that include newlines or tabs should be broken up when generating the IR (ex. template literals in JavaScript may contain those characters). The `ir_helpers::gen_from_string(text)` and `ir_helpers::gen_from_raw_string(text)` (ignores indentation) helpers do this.

The printer will enforce these rules in debug builds.

#### Allocation

Print items are allocated in an `Arena`, which is passed to `PrintItems::new(arena)` and the helpers that create print items. The print items borrow the arena, so the compiler ensures they aren't used after it's dropped.

Create the arena after the data the print items borrow from (ex. the text of the file), so strings from it can be pushed without being copied by using `push_str_runtime_width_computed`. Create an arena for each file that's formatted.

The memory of an arena from `Arena::new()` is freed when it's dropped. To reuse it for the next file instead, store an `ArenaPool` (ex. in the plugin's state) and create the arenas with `pool.create_arena()`, which returns their memory to the pool when they're dropped. The pool keeps the memory until it's cleared or dropped. Wasm plugins are given a pool in `request.arena_pool`.

## Example IR Generation

Given the following AST nodes:

```rust
enum Node<'a> {
  ArrayLiteralExpression(&'a ArrayLiteralExpression),
  ArrayElement(&'a ArrayElement),
}

#[derive(Clone, Copy)]
struct Position {
  /// Line number in the original source code.
  pub line_number: u32,
  /// Column number in the original source code.
  pub column_number: u32,
}

#[derive(Clone)]
struct ArrayLiteralExpression {
  pub position: Position,
  pub elements: Vec<ArrayElement>,
}

#[derive(Clone)]
struct ArrayElement {
  pub position: Position,
  pub text: String,
}
```

With the following expected outputs (when max line width configured in printer is 10):

```
// input
[a   ,   b
    , c
   ]
// output
[a, b, c]

// input
[four, four, four]
// output (since it exceeds the line width of 10)
[
    four,
    four,
    four
]

// input
[
four]
// output (since first element was placed on a different line)
[
    four
]
```

Here's some example IR generation:

```rust
use dprint_core::formatting::*;

pub fn format(expr: &ArrayLiteralExpression) -> String {
  // the arena is created after the expression, so the print items can borrow from it
  let arena = Arena::new();
  let print_items = gen_node(&arena, Node::ArrayLiteralExpression(expr));
  dprint_core::formatting::print(
    print_items,
    PrintOptions {
      indent_width: 4,
      max_width: 10,
      use_tabs: false,
      new_line_text: "\n",
    },
  )
}

// IR generation functions

fn gen_node<'a>(arena: &'a Arena, node: Node<'a>) -> PrintItems<'a> {
  // in a real implementation this function would deal with surrounding comments

  match node {
    Node::ArrayLiteralExpression(expr) => gen_array_literal_expression(arena, expr),
    Node::ArrayElement(array_element) => gen_array_element(arena, array_element),
  }
}

fn gen_array_literal_expression<'a>(arena: &'a Arena, expr: &'a ArrayLiteralExpression) -> PrintItems<'a> {
  let mut items = PrintItems::new(arena);
  let start_ln = LineNumber::new("start");
  let end_ln = LineNumber::new("end");
  let is_multiple_lines = create_is_multiple_lines_resolver(
    arena,
    expr.position,
    // the resolver needs to be `Copy`, so it references the positions in the arena
    arena.bump().alloc_slice_fill_iter(expr.elements.iter().map(|e| e.position)),
    start_ln,
    end_ln,
  );

  // actions::if_column_number_changes is a helper that uses lower level IR to tell when the column number
  // changes at this point
  items.extend(actions::if_column_number_changes(arena, move |context| {
    context.clear_info(end_ln);
  }));

  items.push_info(start_ln);
  items.push_anchor(LineNumberAnchor::new(end_ln)); // updates the line number of end_ln when this changes

  items.push_str("[");
  items.push_condition(conditions::if_true(
    "arrayStartNewLine",
    is_multiple_lines,
    Signal::NewLine.into_print_items(arena),
  ));

  let generated_elements = gen_elements(arena, &expr.elements, is_multiple_lines).into_rc_path();
  items.push_condition(conditions::if_true_or(
    "indentIfMultipleLines",
    is_multiple_lines,
    ir_helpers::with_indent(generated_elements.into_print_items(arena)),
    generated_elements.into_print_items(arena),
  ));

  items.push_condition(conditions::if_true(
    "arrayEndNewLine",
    is_multiple_lines,
    Signal::NewLine.into_print_items(arena),
  ));
  items.push_str("]");

  items.push_info(end_ln);

  return items;

  fn gen_elements<'a>(arena: &'a Arena, elements: &'a [ArrayElement], is_multiple_lines: ConditionResolver<'a>) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    let elements_len = elements.len();

    for (i, elem) in elements.iter().enumerate() {
      items.extend(gen_node(arena, Node::ArrayElement(elem)));

      if i < elements_len - 1 {
        items.push_str(",");
        items.push_condition(conditions::if_true_or(
          "afterCommaSeparator",
          is_multiple_lines,
          Signal::NewLine.into_print_items(arena),
          Signal::SpaceOrNewLine.into_print_items(arena),
        ));
      }
    }

    items
  }
}

fn gen_array_element<'a>(arena: &'a Arena, element: &'a ArrayElement) -> PrintItems<'a> {
  // the text outlives the arena, so it doesn't need to be copied
  element.text.as_str().into_print_items(arena)
}

// helper functions

fn create_is_multiple_lines_resolver<'a>(
  arena: &'a Arena,
  parent_position: Position,
  child_positions: &'a [Position],
  start_ln: LineNumber,
  end_ln: LineNumber,
) -> ConditionResolver<'a> {
  arena.alloc_resolver(move |condition_context| {
    // no items, so format on the same line
    if child_positions.is_empty() {
      return Some(false);
    }
    // first child is on a different line than the start of the parent
    // so format all the children as multi-line
    if parent_position.line_number < child_positions[0].line_number {
      return Some(true);
    }

    // check if it spans multiple lines, and if it does then make it multi-line
    condition_helpers::is_multiple_lines(condition_context, start_ln, end_ln)
  })
}
```
