use dprint_core::formatting::*;

enum Node<'a> {
  ArrayLiteralExpression(&'a ArrayLiteralExpression),
  ArrayElement(&'a ArrayElement),
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
      ArrayElement {
        position: Position {
          line_number: 0,
          column_number: 1,
        },
        text: String::from("test"),
      },
      ArrayElement {
        position: Position {
          line_number: 0,
          column_number: 6,
        },
        text: String::from("other"),
      },
    ],
  };
  do_test(&expr, "[test, other]");
}

#[test]
fn it_formats_as_multi_line_when_first_item_on_different_line_than_expr() {
  let expr = ArrayLiteralExpression {
    position: Position {
      line_number: 0,
      column_number: 0,
    },
    elements: vec![ArrayElement {
      position: Position {
        line_number: 1,
        column_number: 1,
      },
      text: String::from("test"),
    }],
  };
  do_test(&expr, "[\n  test\n]");
}

#[test]
fn it_formats_as_single_line_when_exceeding_print_width_with_only_one_item() {
  let element_text = "asdfasdfasdfasdfasdfasdfasdfasdfasdfasdfasdfsadfasdf";
  let expr = ArrayLiteralExpression {
    position: Position {
      line_number: 0,
      column_number: 0,
    },
    elements: vec![ArrayElement {
      position: Position {
        line_number: 0,
        column_number: 1,
      },
      text: String::from(element_text),
    }],
  };
  do_test(&expr, &format!("[{}]", &element_text));
}

#[test]
fn it_formats_as_multi_line_when_items_exceed_print_width() {
  let expr = ArrayLiteralExpression {
    position: Position {
      line_number: 0,
      column_number: 0,
    },
    elements: vec![
      ArrayElement {
        position: Position {
          line_number: 0,
          column_number: 1,
        },
        text: String::from("test"),
      },
      ArrayElement {
        position: Position {
          line_number: 0,
          column_number: 6,
        },
        text: String::from("other"),
      },
      ArrayElement {
        position: Position {
          line_number: 0,
          column_number: 25,
        },
        text: String::from("asdfasdfasdfasdfasdfasdfasdf"),
      },
    ],
  };
  do_test(&expr, "[\n  test,\n  other,\n  asdfasdfasdfasdfasdfasdfasdf\n]");
}

fn do_test(expr: &ArrayLiteralExpression, expected_text: &str) {
  let result = dprint_core::formatting::format(
    || gen_node(Node::ArrayLiteralExpression(expr)),
    PrintOptions {
      indent_width: 2,
      max_width: 40,
      use_tabs: false,
      new_line_text: "\n",
    },
  );
  assert_eq!(result, expected_text);
}

// IR generation functions

fn gen_node(node: Node) -> PrintItems {
  // in a real implementation this function would deal with surrounding comments

  match node {
    Node::ArrayLiteralExpression(expr) => gen_array_literal_expression(expr),
    Node::ArrayElement(array_element) => gen_array_element(array_element),
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
    ir_helpers::with_indent(generated_elements.into()),
    generated_elements.into(),
  ));

  items.push_condition(conditions::if_true("arrayEndNewLine", is_multiple_lines, Signal::NewLine.into()));
  items.push_str("]");

  items.push_info(end_info);

  return items;

  fn gen_elements(elements: &[ArrayElement], is_multiple_lines: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static)) -> PrintItems {
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
  move |condition_context: &mut ConditionResolverContext| {
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
    condition_resolvers::is_multiple_lines(condition_context, &start_info, &end_info)
  }
}
