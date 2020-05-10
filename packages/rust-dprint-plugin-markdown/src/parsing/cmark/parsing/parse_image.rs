use pulldown_cmark::{LinkType};
use super::super::{ParseError, ast_nodes::*};
use super::char_scanner::CharScanner;
use super::{parse_text_in_brackets, parse_text_in_parens, parse_link_url_and_title};

/// Crudely parses out an image assuming the text is a image.
/// This is done because images have their references inlined by cmark.
pub fn parse_image(offset: usize, text: &str, link_type: LinkType) -> Result<Node, ParseError> {
    let mut char_scanner = CharScanner::new(offset, text);
    let start_pos = char_scanner.pos();

    match link_type {
        LinkType::Inline => parse_inline(start_pos, &mut char_scanner),
        LinkType::Reference | LinkType::ReferenceUnknown | LinkType::Collapsed | LinkType::CollapsedUnknown =>
            parse_reference(start_pos, &mut char_scanner),
        LinkType::Shortcut | LinkType::ShortcutUnknown | LinkType::Email | LinkType::Autolink => Err(
            ParseError::new(Range { start: offset, end: offset}, &format!("Link type not implemented {:?}", link_type))
        ),
    }
}

fn parse_inline(start_pos: usize, char_scanner: &mut CharScanner) -> Result<Node, ParseError> {
    char_scanner.assert_char('!')?;
    char_scanner.assert_char('[')?;
    let text = parse_text_in_brackets(start_pos, char_scanner)?;
    char_scanner.assert_char('(')?;
    let paren_text = parse_text_in_parens(start_pos, char_scanner)?;
    let (url, title) = parse_link_url_and_title(paren_text.trim());

    Ok(InlineImage {
        range: Range { start: start_pos, end: char_scanner.pos() },
        text,
        url,
        title,
    }.into())
}

fn parse_reference(start_pos: usize, char_scanner: &mut CharScanner) -> Result<Node, ParseError> {
    char_scanner.assert_char('!')?;
    char_scanner.assert_char('[')?;
    let text = parse_text_in_brackets(start_pos, char_scanner)?;
    char_scanner.assert_char('[')?;
    let reference = parse_text_in_brackets(start_pos, char_scanner)?;

    Ok(ReferenceImage {
        range: Range { start: start_pos, end: char_scanner.pos() },
        text,
        reference,
    }.into())
}
