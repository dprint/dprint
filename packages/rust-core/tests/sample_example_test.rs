extern crate dprint_core;

use dprint_core::*;

enum Node {
    ArrayLiteralExpression(ArrayLiteralExpression),
    ArrayElement(ArrayElement),
}

#[derive(Clone)]
pub struct Position {
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


#[test]
fn it_formats_when_does_not_exceed_line() {
    let expr = ArrayLiteralExpression {
        position: Position {
            line_number: 0,
            column_number: 0,
        },
        elements: vec![
            ArrayElement { position: Position { line_number: 0, column_number: 1 }, text: String::from("test") },
            ArrayElement { position: Position { line_number: 0, column_number: 6 }, text: String::from("other") },
        ],
    };
    do_test(expr, "[test, other]");
}

#[test]
fn it_formats_as_multi_line_when_first_item_on_different_line_than_expr() {
    let expr = ArrayLiteralExpression {
        position: Position {
            line_number: 0,
            column_number: 0,
        },
        elements: vec![
            ArrayElement { position: Position { line_number: 1, column_number: 1 }, text: String::from("test") }
        ],
    };
    do_test(expr, "[\n  test\n]");
}

#[test]
fn it_formats_as_single_line_when_exceeding_print_width_with_only_one_item() {
    let element_text = "asdfasdfasdfasdfasdfasdfasdfasdfasdfasdfasdfsadfasdf";
    let expr = ArrayLiteralExpression {
        position: Position {
            line_number: 0,
            column_number: 0,
        },
        elements: vec![
            ArrayElement { position: Position { line_number: 0, column_number: 1 }, text: String::from(element_text) }
        ],
    };
    do_test(expr, &format!("[{}]", &element_text));
}

#[test]
fn it_formats_as_multi_line_when_items_exceed_print_width() {
    let expr = ArrayLiteralExpression {
        position: Position {
            line_number: 0,
            column_number: 0,
        },
        elements: vec![
            ArrayElement { position: Position { line_number: 0, column_number: 1 }, text: String::from("test") },
            ArrayElement { position: Position { line_number: 0, column_number: 6 }, text: String::from("other") },
            ArrayElement { position: Position { line_number: 0, column_number: 25 }, text: String::from("asdfasdfasdfasdfasdfasdfasdf") },
        ],
    };
    do_test(expr, "[\n  test,\n  other,\n  asdfasdfasdfasdfasdfasdfasdf\n]");
}

fn do_test(expr: ArrayLiteralExpression, expected_text: &str) {
    let print_items = parse_node(Node::ArrayLiteralExpression(expr));
    let write_items = dprint_core::get_write_items(print_items, GetWriteItemsOptions {
        indent_width: 2,
        max_width: 40,
        is_testing: true,
    });
    let result = print_write_items(write_items, PrintWriteItemsOptions {
        use_tabs: false,
        newline_kind: "\n",
        indent_width: 2
    });
    assert_eq!(result, expected_text);
}

// node parsing functions

fn parse_node(node: Node) -> Vec<PrintItem> {
    // in a real implementation this function would deal with surrounding comments

    match node {
        Node::ArrayLiteralExpression(expr) => parse_array_literal_expression(&expr),
        Node::ArrayElement(array_element) => parse_array_element(&array_element),
    }
}

fn parse_array_literal_expression(expr: &ArrayLiteralExpression) -> Vec<PrintItem> {
    let mut items: Vec<PrintItem> = Vec::new();
    let start_info = Info::new("start");
    let end_info = Info::new("end");
    let is_multiple_lines = create_is_multiple_lines_resolver(
        expr.position.clone(),
        expr.elements.iter().map(|e| e.position.clone()).collect(),
        &start_info,
        &end_info
    );

    items.push(start_info.into());

    items.push("[".into());
    items.push(if_true(is_multiple_lines.clone(), PrintItem::NewLine));

    let parsed_elements = parse_elements(&expr.elements, &is_multiple_lines);
    items.push(Condition::new("indentIfMultipleLines", ConditionProperties {
        condition: Box::new(is_multiple_lines.clone()),
        true_path: Some(with_indent(parsed_elements.clone())),
        false_path: Some(parsed_elements),
    }).into());

    items.push(if_true(is_multiple_lines, PrintItem::NewLine));
    items.push("]".into());

    items.push(end_info.into());

    return items;

    fn parse_elements(
        elements: &Vec<ArrayElement>,
        is_multiple_lines: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static)
    ) -> Vec<PrintItem> {
        let mut items = Vec::new();

        for i in 0..elements.len() {
            items.extend_from_slice(&parse_node(Node::ArrayElement(elements[i].clone())));

            if i < elements.len() - 1 {
                items.push(",".into());
                items.push(if_true_or(
                    is_multiple_lines.clone(),
                    PrintItem::NewLine,
                    PrintItem::SpaceOrNewLine
                ));
            }
        }

        items
    }
}

fn parse_array_element(element: &ArrayElement) -> Vec<PrintItem> {
    vec![(&element.text).into()]
}

// helper functions

fn create_is_multiple_lines_resolver(
    parent_position: Position,
    child_positions: Vec<Position>,
    start_info: &Info,
    end_info: &Info
) -> impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static {
    let captured_start_info = start_info.clone();
    let captured_end_info = end_info.clone();

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
        let resolved_start_info = condition_context.get_resolved_info(&captured_start_info).unwrap();
        let optional_resolved_end_info = condition_context.get_resolved_info(&captured_end_info);

        optional_resolved_end_info.map(|resolved_end_info| {
            resolved_start_info.line_number < resolved_end_info.line_number
        })
    };
}

fn with_indent(mut elements: Vec<PrintItem>) -> Vec<PrintItem> {
    elements.insert(0, PrintItem::StartIndent);
    elements.push(PrintItem::FinishIndent);
    elements
}

fn if_true(
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_item: PrintItem
) -> PrintItem {
    Condition::new("", ConditionProperties {
        true_path: Some(vec![true_item]),
        false_path: Option::None,
        condition: Box::new(resolver.clone()),
    }).into()
}

fn if_true_or(
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_item: PrintItem,
    false_item: PrintItem
) -> PrintItem {
    Condition::new("", ConditionProperties {
        true_path: Some(vec![true_item]),
        false_path: Some(vec![false_item]),
        condition: Box::new(resolver.clone())
    }).into()
}
