use dprint_core::*;
use dprint_core::{parser_helpers::*, condition_resolvers};
use super::ast_nodes::*;
use super::parser_types::*;

fn parse_source_file(source_file: &SourceFile, context: &mut Context) -> PrintItems {
    println!("Children: {}", source_file.children.len());

    let mut items = parse_nodes(&source_file.children, context);

    items.push_condition(if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        Signal::NewLine.into()
    ));

    items
}

pub fn parse_node(node: &Node, context: &mut Context) -> PrintItems {
    match node {
        Node::SourceFile(node) => parse_source_file(node, context),
        Node::Heading(node) => parse_heading(node, context),
        Node::Paragraph(node) => parse_paragraph(node, context),
        Node::CodeBlock(node) => parse_code_block(node, context),
        Node::Code(node) => parse_code(node, context),
        Node::Text(node) => parse_text(node, context),
        Node::SoftBreak(_) => PrintItems::new(),
        Node::HardBreak(_) => Signal::NewLine.into(),
        Node::NotImplemented(_) => parse_raw_string(node.text(context)),
    }
}

fn parse_nodes(nodes: &Vec<Node>, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();
    let mut last_node: Option<&Node> = None;

    for node in nodes {
        // todo: this area needs to be thought out more
        let is_current_soft_break = match node { Node::SoftBreak(_) => true, _=> false, };
        if let Some(last_node) = last_node {
            match last_node {
                Node::Heading(_) | Node::Paragraph(_) | Node::CodeBlock(_) => {
                    items.push_signal(Signal::NewLine);
                    items.push_signal(Signal::NewLine);
                },
                Node::Text(text) => {
                    if text.ends_with_whitespace() {
                        items.push_signal(Signal::SpaceOrNewLine);
                    } else if let Node::Text(text) = node {
                        if text.starts_with_whitespace() {
                            items.push_signal(Signal::SpaceOrNewLine);
                        }
                    }
                },
                Node::SoftBreak(_) => {
                    if !is_current_soft_break {
                        items.push_signal(Signal::SpaceOrNewLine);
                    }
                },
                Node::Code(_) => {
                    if !is_current_soft_break {
                        items.push_signal(Signal::SpaceOrNewLine);
                    }
                },
                _ => {},
            }
        }

        items.extend(parse_node(node, context));
        last_node = Some(node);
    }

    items
}

fn parse_heading(heading: &Heading, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();

    items.push_str(&format!("{} ", "#".repeat(heading.level as usize)));
    items.extend(with_no_new_lines(parse_nodes(&heading.children, context)));

    items
}

fn parse_paragraph(paragraph: &Paragraph, context: &mut Context) -> PrintItems {
    parse_nodes(&paragraph.children, context)
}

fn parse_code_block(code_block: &CodeBlock, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();
    let indent_level = context.get_indent_level_at_pos(code_block.range.start);

    // header
    if indent_level == 0 {
        items.push_str("```");
        if let Some(tag) = &code_block.tag {
            items.push_str(tag);
        }
        items.push_signal(Signal::NewLine);
    }

    // body
    items.extend(parser_helpers::parse_string(&code_block.code.trim()));

    // footer
    if indent_level == 0 {
        items.push_signal(Signal::NewLine);
        items.push_str("```");
    }

    return with_indent_times(items, indent_level);
}

fn parse_code(code: &Code, _: &mut Context) -> PrintItems {
    format!("`{}`", code.code.trim()).into()
}

fn parse_text(text: &Text, _: &mut Context) -> PrintItems {
    let mut text_builder = TextBuilder::new();

    for c in text.text.chars() {
        if c.is_whitespace() {
            text_builder.space_or_new_line();
        } else {
            text_builder.add_char(c);
        }
    }

    return text_builder.build();

    struct TextBuilder {
        items: PrintItems,
        was_last_whitespace: bool,
        current_word: Option<String>,
    }

    impl TextBuilder {
        pub fn new() -> TextBuilder {
            TextBuilder {
                items: PrintItems::new(),
                was_last_whitespace: false,
                current_word: None,
            }
        }

        pub fn build(mut self) -> PrintItems {
            self.flush_current_word();
            self.items
        }

        pub fn space_or_new_line(&mut self) {
            if self.items.is_empty() && self.current_word.is_none() { return; }
            if self.was_last_whitespace { return; }

            self.flush_current_word();

            self.was_last_whitespace = true;
        }

        pub fn add_char(&mut self, character: char) {
            if self.was_last_whitespace {
                self.items.push_signal(Signal::SpaceOrNewLine);
                self.was_last_whitespace = false;
            }

            if let Some(current_word) = self.current_word.as_mut() {
                current_word.push(character);
            } else {
                let mut text = String::new();
                text.push(character);
                self.current_word = Some(text);
            }
        }

        fn flush_current_word(&mut self) {
            if let Some(current_word) = self.current_word.take() {
                self.items.push_str(&current_word);
            }
        }
    }
}