use pulldown_cmark::*;
use super::ast_nodes::*;
use super::parsing::{parse_link_reference_definitions, parse_link as parse_link_from_text, parse_image as parse_image_from_text};

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
    file_text: &'a str,
    last_range: Range,
}

impl<'a> EventIterator<'a> {
    pub fn new(file_text: &'a str, iterator: OffsetIter<'a>) -> EventIterator<'a> {
        EventIterator {
            file_text,
            iterator,
            last_range: Range {
                start: 0,
                end: 0
            }
        }
    }

    pub fn next(&mut self) -> Option<Event<'a>> {
        if let Some((event, range)) = self.iterator.next() {
            println!("Event: {:?}", event);
            println!("Range: {:?}", range);
            self.last_range = range;
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
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut children: Vec<Node> = Vec::new();
    let mut iterator = EventIterator::new(file_text, Parser::new_ext(file_text, options).into_offset_iter());
    let mut last_event_range: Option<Range> = None;

    while let Some(event) = iterator.next() {
        let current_range = iterator.get_last_range();
        if let Some(references) = parse_references(&last_event_range, current_range.start, &mut iterator)? {
            children.push(references);
        }

        children.push(parse_event(event, &mut iterator)?);
        last_event_range = Some(current_range);
    }

    if let Some(references) = parse_references(&last_event_range, file_text.len(), &mut iterator)? {
        children.push(references);
    }

    Ok(SourceFile {
        children,
        range: iterator.get_range_for_start(0),
    })
}

fn parse_references(last_event_range: &Option<Range>, end: usize, iterator: &mut EventIterator) -> Result<Option<Node>, ParseError> {
    if let Some(last_event_range) = last_event_range {
        let references = parse_link_reference_definitions(last_event_range.end, &iterator.file_text[last_event_range.end..end])?;
        if !references.is_empty() {
            return Ok(Some(Paragraph {
                range: Range { start: references.first().unwrap().range.start, end: references.last().unwrap().range.end },
                children: references.into_iter().map(|x| x.into()).collect(),
            }.into()));
        }
    }

    Ok(None)
}

fn parse_event(event: Event, iterator: &mut EventIterator) -> Result<Node, ParseError> {
    match event {
        Event::Start(tag) => parse_start(tag, iterator),
        Event::End(_) => Ok(iterator.get_not_implemented()), // do nothing
        Event::Code(code) => parse_code(code, iterator).map(|x| x.into()),
        Event::Text(text) => parse_text(text, iterator).map(|x| x.into()),
        Event::Html(html) => parse_html(html, iterator).map(|x| x.into()),
        Event::FootnoteReference(reference) => parse_footnote_reference(reference, iterator).map(|x| x.into()),
        Event::SoftBreak => Ok(SoftBreak { range: iterator.get_last_range() }.into()),
        Event::HardBreak => Ok(HardBreak { range: iterator.get_last_range() }.into()),
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
        Tag::FootnoteDefinition(label) => parse_footnote_definition(label, iterator).map(|x| x.into()),
        Tag::Table(text_alignment) => Ok(iterator.get_not_implemented()),
        Tag::TableHead => Ok(iterator.get_not_implemented()),
        Tag::TableRow => Ok(iterator.get_not_implemented()),
        Tag::TableCell => Ok(iterator.get_not_implemented()),
        Tag::Emphasis => parse_text_decoration(TextDecorationKind::Emphasis, iterator).map(|x| x.into()),
        Tag::Strong => parse_text_decoration(TextDecorationKind::Strong, iterator).map(|x| x.into()),
        Tag::Strikethrough => parse_text_decoration(TextDecorationKind::Strikethrough, iterator).map(|x| x.into()),
        Tag::Link(link_type, _, _) => parse_link(link_type, iterator),
        Tag::Image(link_type, _, _) => parse_image(link_type, iterator),
        Tag::List(first_item_number) => parse_list(first_item_number, iterator).map(|x| x.into()),
        Tag::Item => parse_item(iterator).map(|x| x.into()),
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
    let text = text.as_ref();
    let trimmed_text = text.trim();
    let start = iterator.get_last_range().start + (text.len() - text.trim_start().len());

    Ok(Text {
        range: Range { start, end: start + trimmed_text.len() },
        text: String::from(trimmed_text),
    })
}

fn parse_text_decoration(kind: TextDecorationKind, iterator: &mut EventIterator) -> Result<TextDecoration, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Emphasis) => break,
            Event::End(Tag::Strikethrough) => break,
            Event::End(Tag::Strong) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(TextDecoration {
        range: iterator.get_range_for_start(start),
        kind,
        children,
    })
}

fn parse_html(text: CowStr, iterator: &mut EventIterator) -> Result<Html, ParseError> {
    let text = String::from(text.as_ref().trim_end());
    let start = iterator.get_last_range().start;
    Ok(Html {
        range: Range { start, end: start + text.len() },
        text,
    })
}

fn parse_footnote_reference(name: CowStr, iterator: &mut EventIterator) -> Result<FootnoteReference, ParseError> {
    Ok(FootnoteReference {
        range: iterator.get_last_range(),
        name: String::from(name.as_ref()),
    })
}

fn parse_footnote_definition(name: CowStr, iterator: &mut EventIterator) -> Result<FootnoteDefinition, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::FootnoteDefinition(_)) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(FootnoteDefinition {
        range: iterator.get_range_for_start(start),
        name: String::from(name.as_ref()),
        children,
    })
}

fn parse_link(link_type: LinkType, iterator: &mut EventIterator) -> Result<Node, ParseError> {
    let start = iterator.start();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Link(_, _, _)) => break,
            _ => {}, // ignore link children
        }
    }

    // iterator.get_last_range().end in pulldown-cmark is wrong, so just pass all the text from the start (issue #430 in their repo)
    parse_link_from_text(start, &iterator.file_text[start..], link_type)
}

fn parse_image(link_type: LinkType, iterator: &mut EventIterator) -> Result<Node, ParseError> {
    let start = iterator.start();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Image(_, _, _)) => break,
            _ => {}, // ignore link children
        }
    }

    parse_image_from_text(start, &iterator.file_text[start..], link_type)
}

fn parse_list(start_index: Option<u64>, iterator: &mut EventIterator) -> Result<List, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::List(_)) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(List {
        range: iterator.get_range_for_start(start),
        start_index,
        children,
    })
}

fn parse_item(iterator: &mut EventIterator) -> Result<Item, ParseError> {
    let start = iterator.start();
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Item) => break,
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(Item {
        range: iterator.get_range_for_start(start),
        children,
    })
}
