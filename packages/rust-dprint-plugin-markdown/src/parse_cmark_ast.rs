use pulldown_cmark::*;
use super::ast_nodes::*;

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
            Some(event)
        } else {
            None
        }
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

pub fn parse_cmark_ast(file_text: &str) -> Result<SourceFile, String> {
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

fn parse_event(event: Event, iterator: &mut EventIterator) -> Result<Node, String> {
    println!("Event: {:?}", event);

    match event {
        Event::Start(tag) => parse_start(tag, iterator),
        Event::End(tag) => Ok(iterator.get_not_implemented()),
        Event::Code(code) => Ok(iterator.get_not_implemented()),
        Event::Text(text) => parse_text(text, iterator),
        Event::Html(html) => Ok(iterator.get_not_implemented()),
        Event::FootnoteReference(reference) => Ok(iterator.get_not_implemented()),
        Event::SoftBreak => Ok(SoftBreak { range: iterator.get_last_range() }.into()),
        Event::HardBreak => Ok(SoftBreak { range: iterator.get_last_range() }.into()),
        Event::Rule => Ok(iterator.get_not_implemented()),
        Event::TaskListMarker(is_checked) => Ok(iterator.get_not_implemented()),
    }
}

fn parse_start(start_tag: Tag, iterator: &mut EventIterator) -> Result<Node, String> {
    match start_tag {
        Tag::Paragraph => parse_paragraph(iterator),
        Tag::Heading(level) => parse_heading(level, iterator),
        Tag::BlockQuote => Ok(iterator.get_not_implemented()),
        Tag::CodeBlock(code_block) => Ok(iterator.get_not_implemented()),
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

fn parse_heading(level: u32, iterator: &mut EventIterator) -> Result<Node, String> {
    let start = iterator.last_range.start;
    let mut children = Vec::new();

    while let Some(event) = iterator.next() {
        match event {
            Event::End(Tag::Heading(end_level)) => {
                if end_level == level { break; }
                return Err(format!("Found end tag with level {}, but expected {}", end_level, level));
            },
            _ => children.push(parse_event(event, iterator)?),
        }
    }

    Ok(Heading {
        range: iterator.get_range_for_start(start),
        level,
        children,
    }.into())
}

fn parse_paragraph(iterator: &mut EventIterator) -> Result<Node, String> {
    let start = iterator.last_range.start;
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
    }.into())
}

fn parse_text(text: CowStr, iterator: &mut EventIterator) -> Result<Node, String> {
    Ok(Text {
        range: iterator.get_last_range(),
        text: String::from(text.as_ref()),
    }.into())
}
