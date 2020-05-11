# dprint-core

[![](https://img.shields.io/crates/v/dprint-core.svg)](https://crates.io/crates/dprint-core)

Rust crate to help build a code formatter.

## Api

Use:

```rust
let print_items = ...; // parsed out IR (see example below)
let result = dprint_core::print(print_items, PrintOptions {
    indent_width: 4,
    max_width: 10,
    use_tabs: false,
    newline_kind: "\n",
});
```

## Example

This reimplements the example from [overview.md](../../docs/overview.md), but in Rust.

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

```ts
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
extern crate dprint_core;

use dprint_core::*;

pub fn format(expr: &ArrayLiteralExpression) -> String {
    // parse out the print items from the AST
    let print_items = parse_node(Node::ArrayLiteralExpression(expr));

    // print them
    dprint_core::print(print_items, PrintOptions {
        indent_width: 4,
        max_width: 10,
        use_tabs: false,
        newline_kind: "\n",
    })
}

// node parsing functions

fn parse_node(node: Node) -> PrintItems {
    // in a real implementation this function would deal with surrounding comments

    match node {
        Node::ArrayLiteralExpression(expr) => parse_array_literal_expression(&expr),
        Node::ArrayElement(array_element) => parse_array_element(&array_element),
    }
}

fn parse_array_literal_expression(expr: &ArrayLiteralExpression) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("start");
    let end_info = Info::new("end");
    let is_multiple_lines = create_is_multiple_lines_resolver(
        expr.position.clone(),
        expr.elements.iter().map(|e| e.position.clone()).collect(),
        start_info,
        end_info
    );

    items.push_info(start_info);

    items.push_str("[");
    items.push_condition(conditions::if_true(
        "arrayStartNewLine",
        is_multiple_lines.clone(),
        Signal::NewLine.into()
    ));

    let parsed_elements = parse_elements(&expr.elements, &is_multiple_lines).into_rc_path();
    items.push_condition(conditions::if_true_or(
        "indentIfMultipleLines",
        is_multiple_lines.clone(),
        parser_helpers::with_indent(parsed_elements.clone().into()),
        parsed_elements.into(),
    ));

    items.push_condition(conditions::if_true(
        "arrayEndNewLine",
        is_multiple_lines,
        Signal::NewLine.into()
    ));
    items.push_str("]");

    items.push_info(end_info);

    return items;

    fn parse_elements(
        elements: &Vec<ArrayElement>,
        is_multiple_lines: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static)
    ) -> PrintItems {
        let mut items = PrintItems::new();
        let elements_len = elements.len();

        for (i, elem) in elements.iter().enumerate() {
            items.extend(parse_node(Node::ArrayElement(elem)));

            if i < elements_len - 1 {
                items.push_str(",");
                items.push_condition(conditions::if_true_or(
                    "afterCommaSeparator",
                    is_multiple_lines.clone(),
                    Signal::NewLine.into(),
                    Signal::SpaceOrNewLine.into()
                ));
            }
        }

        items
    }
}

fn parse_array_element(element: &ArrayElement) -> PrintItems {
    (&element.text).into()
}

// helper functions

fn create_is_multiple_lines_resolver(
    parent_position: Position,
    child_positions: Vec<Position>,
    start_info: Info,
    end_info: Info
) -> impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static {
    // todo: this could be more efficient only only use references and avoid the clones
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
