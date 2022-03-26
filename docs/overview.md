# Overview

**NOTE**: This document is out of date, but outlines the basic idea of how it works.

1. Source code is parsed to an AST (recommended, but not required).
2. AST is traversed and IR is generated.
3. IR is printed by printer.

## IR Generation

The immediate representation describes how the nodes should be formatted. It consists of...

1. Texts
2. Infos
3. Conditions
4. Signals

These are referred to as "print items" in the code.

### Texts

Strings that the printer should print. For example `"async"`.

### Infos

These objects are invisible in the output. They may be placed into the IR and when resolved by the printer, report the following information about where the info ended up at:

- `lineNumber`
- `columnNumber`
- `indentLevel`
- `lineStartIndentLevel`
- `lineStartColumnNumber`

### Conditions

Conditions have three main properties:

- Optional true path - Print items to use when the condition is resolved as _true_.
- Optional false path - Print items to use when the condition is resolved as _false_.
- Condition resolver - Function or condition that the printer uses to resolve the condition as _true_ or _false_.

#### Condition Resolver

Conditions are usually resolved by looking at the value of a resolved info, other condition, or based on the original AST node.

The infos & conditions that are inspected may appear before or even after the condition.

### Signals

This is an enum that signals information to the printer.

- `NewLine` - Signal that a new line should occur based on the printer settings.
- `Tab` - Signal that a tab should occur based on the printer settings (ex. if indent width is 4 it will increase the column width by 4 for each tab).
- `PossibleNewLine` - Signal that the current location could be a newline when exceeding the line width.
- `SpaceOrNewLine` - Signal that the current location should be a space, but could be a newline if exceeding the line width.
- `ExpectNewLine` - Expect the next character to be a newline. If it's not, force a newline. This is useful to use at the end of single line comments in JS, for example.
- `StartIndent` - Signal the start of a section that should be indented.
- `FinishIndent` - Signal the end of a section that should be indented.
- `StartNewLineGroup` - Signal the start of a group of print items that have a lower precedence for being broken up with a newline for exceeding the line width.
- `FinishNewLineGroup` - Signal the end of a newline group.
- `SingleIndent` - Signal that a single indent should occur based on the printer settings (ex. prints a tab when using tabs).
- `StartIgnoringIndent` - Signal to the printer that it should stop using indentation.
- `FinishIgnoringIndent` - Signal to the printer that it should start using indentation again.

## Printer

The printer takes the IR and outputs the final code. Its main responsibilities are:

1. Resolving infos and conditions in the IR.
2. Printing out the text with the correct indentation and newline kind.
3. Seeing where lines exceed the maximum line width and breaking up the line as specified in the IR.

#### Rules

The printer never checks the contents of the provided strings—it only looks at the length of the strings. For that reason there are certain rules:

1. Never use a tab in a string. Instead, use `Signal.Tab` (see _Signals_ below). Tabs increase the column width based on the indent width and need to be treated differently.
2. Never use a newline in a string. Instead use `Signal.NewLine`.

Strings that include newlines or tabs should be broken up when parsed (ex. template literals in JavaScript may contain those characters).

The printer will enforce these rules in non-release mode.

## Example IR Generation

Given the following AST nodes:

```rust
enum Node<'a> {
  ArrayLiteralExpression(&'a ArrayLiteralExpression),
  ArrayElement(&'a ArrayElement),
}

#[derive(Clone)]
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
  dprint_core::formatting::format(
    || gen_node(Node::ArrayLiteralExpression(expr)),
    PrintOptions {
      indent_width: 4,
      max_width: 10,
      use_tabs: false,
      newline_kind: "\n",
    },
  )
}

// IR generation functions

fn gen_node(node: Node) -> PrintItems {
  // in a real implementation this function would deal with surrounding comments

  match node {
    Node::ArrayLiteralExpression(expr) => gen_array_literal_expression(&expr),
    Node::ArrayElement(array_element) => gen_array_element(&array_element),
  }
}

fn gen_array_literal_expression(expr: &ArrayLiteralExpression) -> PrintItems {
  let mut items = PrintItems::new();
  let start_info = Info::new("start");
  let end_info = Info::new("end");
  let is_multiple_lines = create_is_multiple_lines_resolver(
    expr.position.clone(),
    expr.elements.iter().map(|e| e.position.clone()).collect(),
    start_info,
    end_info,
  );

  items.push_info(start_info);

  items.push_str("[");
  items.push_condition(conditions::if_true("arrayStartNewLine", is_multiple_lines.clone(), Signal::NewLine.into()));

  let generated_elements = gen_elements(&expr.elements, &is_multiple_lines).into_rc_path();
  items.push_condition(conditions::if_true_or(
    "indentIfMultipleLines",
    is_multiple_lines.clone(),
    ir_helpers::with_indent(generated_elements.clone().into()),
    generated_elements.into(),
  ));

  items.push_condition(conditions::if_true("arrayEndNewLine", is_multiple_lines, Signal::NewLine.into()));
  items.push_str("]");

  items.push_info(end_info);

  return items;

  fn gen_elements(elements: &Vec<ArrayElement>, is_multiple_lines: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static)) -> PrintItems {
    let mut items = PrintItems::new();
    let elements_len = elements.len();

    for (i, elem) in elements.iter().enumerate() {
      items.extend(gen_node(Node::ArrayElement(elem)));

      if i < elements_len - 1 {
        items.push_str(",");
        items.push_condition(conditions::if_true_or(
          "afterCommaSeparator",
          is_multiple_lines.clone(),
          Signal::NewLine.into(),
          Signal::SpaceOrNewLine.into(),
        ));
      }
    }

    items
  }
}

fn gen_array_element(element: &ArrayElement) -> PrintItems {
  element.text.to_string().into()
}

// helper functions

fn create_is_multiple_lines_resolver(
  parent_position: Position,
  child_positions: Vec<Position>,
  start_info: Info,
  end_info: Info,
) -> impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static {
  // This could be more efficient by only using references and avoid clones
  // I'm too lazy to update this sample, but it should help you get the idea.
  return move |condition_context: &mut ConditionResolverContext| {
    // no items, so format on the same line
    if child_positions.len() == 0 {
      return Some(false);
    }
    // first child is on a different line than the start of the parent
    // so format all the children as multi-line
    if parent_position.line_number < child_positions[0].line_number {
      return Some(true);
    }

    // check if it spans multiple lines, and if it does then make it multi-line
    condition_resolvers::is_multiple_lines(condition_context, &start_info, &end_info)
  };
}
```
