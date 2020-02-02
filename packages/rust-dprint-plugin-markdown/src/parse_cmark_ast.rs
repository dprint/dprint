use pulldown_cmark::*;
use super::ast_nodes::*;

pub struct ParseError {
    /// This range the parse error occurred.
    pub range: Range,
    /// The associated error message.
    pub message: String,
}

impl ParseError {
    pub(super) fn new(range: Range, message: &str) -> ParseError {
        ParseError { range, message: String::from(message) }
    }
}

struct EventIterator<'a> {
    iterator: OffsetIter<'a>,
    last_range: Range,
}

impl<'a> EventIterator<'a> {
    pub fn new(iterator: OffsetIter<'a>) -> EventIterator<'a> {
        EventIterator {
            iterator,
            last_range: Range {
                start: 0,
                end: 0
            }
        }
    }

    pub fn next(&mut self) -> Option<Event<'a>> {
        if let Some((event, range)) = self.iterator.next() {
            self.last_range = range;
            println!("Event: {:?}", event);
            Some(event)
        } else {
            None
        }
    }

    pub fn start(&self) -> usize {
        self.last_range.start
    }

    pub fn get_range_for_start(&self, start: usize) -> Range {
        Range {
            start,
            end: self.last_range.end,
        }
    }

    pub fn get_last_range(&self) -> Range {
        self.last_range.clone()
    }

    pub fn get_not_implemented(&self) -> Node {
        NotImplemented {
            range: self.last_range.clone(),
        }.into()
    }
}

pub fn parse_cmark_ast(file_text: &str) -> Result<SourceFile, ParseError> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut children: Vec<Node> = Vec::new();
    let mut iterator = EventIterator::new(Parser::new_ext(file_text, options).into_offset_iter());

    while let Some(event) = iterator.next() {
        children.push(parse_event(event, &mut iterator)?);
    }

    Ok(SourceFile {
        children,
        range: iterator.get_range_for_start(0),
    })
}

fn parse_event(event: Event, iterator: &mut EventIterator) -> Result<Node, ParseError> {
    match event {
        Event::Start(tag) => parse_start(tag, iterator),
        Event::End(_) => Ok(iterator.get_not_implemented()), // do nothing
        Event::Code(code) => parse_code(code, iterator).map(|x| x.into()),
        Event::Text(text) => parse_text(text, iterator).map(|x| x.into()),
        Event::Html(html) => Ok(iterator.get_not_implemented()),
        Event::FootnoteReference(reference) => Ok(iterator.get_not_implemented()),
        Event::SoftBreak => Ok(SoftBreak { range: iterator.get_last_range() }.into()),
        Event::HardBreak => Ok(SoftBreak { range: iterator.get_last_range() }.into()),
        Event::Rule => Ok(iterator.get_not_implemented()),
        Event::TaskListMarker(is_checked) => Ok(iterator.get_not_implemented()),
    }
}

fn parse_start(start_tag: Tag, iterator: &mut EventIterator) -> Result<Node, ParseError> {
    match start_tag {
        Tag::Heading(level) => parse_heading(level, iterator).map(|x| x.into()),
        Tag::Paragraph => parse_paragraph(iterator).map(|x| x.into()),
        Tag::BlockQuote => parse_block_quote(iterator).map(|x| x.into()),
        Tag::CodeBlock(tag) => parse_code_block(tag, iterator).map(|x| x.into()),
        Tag::List(first_item_number) => Ok(iterator.get_not_implemented()),
        Tag::Item => Ok(iterator.get_not_implemented()),
        Tag::FootnoteDefinition(label) => Ok(iterator.get_not_implemented()),
        Tag::Table(text_alignment) => Ok(iterator.get_not_implemented()),
        Tag::TableHead => Ok(iterator.get_not_implemented()),
        Tag::TableRow => Ok(iterator.get_not_implemented()),
        Tag::TableCell => Ok(iterator.get_not_implemented()),
        Tag::Emphasis => Ok(iterator.get_not_implemented()),
        Tag::Strong => Ok(iterator.get_not_implemented()),
        Tag::Strikethrough => Ok(iterator.get_not_implemented()),
        Tag::Link(link_type, destination_url, title) => Ok(iterator.get_not_implemented()),
        Tag::Image(link_type, destination_url, title) => Ok(iterator.get_not_implemented()),
    }
}

fn parse_heading(level: u32, iterator: &mut EventIterator) -> Result<Heading, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Heading(end_level)) => {
                if end_level == level { break; }
                return Err(ParseError::new(
                    iterator.get_last_range(),
                    &format!("Found end tag with level {}, but expected {}", end_level, level)
                ));
            },
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(Heading {
        range: iterator.get_range_for_start(start),
        level,
        children,
    })
}

fn parse_paragraph(iterator: &mut EventIterator) -> Result<Paragraph, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Paragraph) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(Paragraph {
        range: iterator.get_range_for_start(start),
        children,
    })
}

fn parse_block_quote(iterator: &mut EventIterator) -> Result<BlockQuote, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::BlockQuote) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(BlockQuote {
        range: iterator.get_range_for_start(start),
        children,
    })
}

fn parse_code_block(tag: CowStr, iterator: &mut EventIterator) -> Result<CodeBlock, ParseError> {
    let start = iterator.start();
    let mut code = String::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::CodeBlock(_)) => break,
            Event::Text(event_text) => code.push_str(event_text.as_ref()),
            _ => return Err(ParseError::new(iterator.get_last_range(), "Unexpected event found when parsing code block.")),
        }
    }

    let tag = String::from(tag.as_ref().trim());
    let tag = if tag.is_empty() { None } else { Some(tag) };

    Ok(CodeBlock {
        range: iterator.get_range_for_start(start),
        tag,
        code,
    })
}

fn parse_code(code: CowStr, iterator: &mut EventIterator) -> Result<Code, ParseError> {
    Ok(Code {
        range: iterator.get_last_range(),
        code: String::from(code.as_ref()),
    })
}

fn parse_text(text: CowStr, iterator: &mut EventIterator) -> Result<Text, ParseError> {
    Ok(Text {
        range: iterator.get_last_range(),
        text: String::from(text.as_ref()),
    })
}
