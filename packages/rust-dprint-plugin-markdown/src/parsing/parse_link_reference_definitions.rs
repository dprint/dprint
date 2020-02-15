use super::super::{ParseError, ast_nodes::{Range, LinkReference}};
use super::char_scanner::CharScanner;
use super::{parse_text_in_brackets, parse_link_url_and_title};

/// Crudely parses out link reference definitions from the provided text.
pub fn parse_link_reference_definitions(offset: usize, text: &str) -> Result<Vec<LinkReference>, ParseError> {
    let mut char_scanner = CharScanner::new(offset, text);
    let mut references = Vec::new();

    while let Some((byte_pos, c)) = char_scanner.next() {
        if c.is_whitespace() {
            continue;
        } else if c == '[' {
            let link_ref_definition = parse_link_reference_definition(byte_pos, &mut char_scanner)?;
            references.push(link_ref_definition);
        } else {
            return Err(ParseError::new(
                Range { start: byte_pos, end: byte_pos },
                &format!("Unexpected token `{}` while parsing link reference definition.", c)
            ));
        }
    }

    return Ok(references);
}

fn parse_link_reference_definition(start_pos: usize, char_scanner: &mut CharScanner) -> Result<LinkReference, ParseError> {
    let name = parse_text_in_brackets(start_pos, char_scanner)?;
    char_scanner.assert_char(':')?;
    char_scanner.skip_spaces();
    let final_text = parse_reference_link(start_pos, char_scanner)?;
    let (url, title) = parse_link_url_and_title(final_text.trim());

    Ok(LinkReference {
        range: Range { start: start_pos, end: char_scanner.pos() },
        name,
        link: url,
        title,
    })
}

fn parse_reference_link(start_pos: usize, char_scanner: &mut CharScanner) -> Result<String, ParseError> {
    let mut reference_link = String::new();
    while let Some((byte_pos, c)) = char_scanner.next() {
        match c {
            '\n' => break,
            '[' => return Err(ParseError::new(
                Range { start: byte_pos, end: byte_pos },
                "Unexpected open bracket parsing link reference definition link."
            )),
            _ => reference_link.push(c),
        }
    }

    if reference_link.is_empty() {
        return Err(ParseError::new(
            Range { start: start_pos, end: char_scanner.pos() },
            "Unexpected empty link parsing link reference definition link."
        ));
    }

    return Ok(reference_link);
}

#[cfg(test)]
mod tests {
    use super::{parse_link_reference_definitions};

    #[test]
    fn it_parses_empty_string() {
        let result = parse_link_reference_definitions(10, "");
        assert_eq!(result.is_ok(), true);
        assert_eq!(result.ok().unwrap().is_empty(), true);
    }

    #[test]
    fn it_finds_link_reference() {
        let result = parse_link_reference_definitions(10, "[Some reference]: https://dprint.dev");
        assert_eq!(result.is_ok(), true);
        let references = result.ok().unwrap();
        assert_eq!(references.len(), 1);
        let reference = &references[0];
        assert_eq!(reference.range.start, 10);
        assert_eq!(reference.range.end, 46);
        assert_eq!(reference.name, "Some reference");
        assert_eq!(reference.link, "https://dprint.dev");
    }

    #[test]
    fn it_finds_link_reference_with_new_line_after() {
        let result = parse_link_reference_definitions(10, "[Some reference]: https://dprint.dev\n");
        assert_eq!(result.is_ok(), true);
        let references = result.ok().unwrap();
        assert_eq!(references.len(), 1);
        let reference = &references[0];
        assert_eq!(reference.range.start, 10);
        assert_eq!(reference.range.end, 46);
        assert_eq!(reference.name, "Some reference");
        assert_eq!(reference.link, "https://dprint.dev");
    }

    #[test]
    fn it_finds_multiple_link_references() {
        let result = parse_link_reference_definitions(10, "[Some reference]: https://dprint.dev\n\n[other]: https://github.com");
        assert_eq!(result.is_ok(), true);
        let references = result.ok().unwrap();
        assert_eq!(references.len(), 2);
        let reference = &references[0];
        assert_eq!(reference.range.start, 10);
        assert_eq!(reference.range.end, 46);
        assert_eq!(reference.name, "Some reference");
        assert_eq!(reference.link, "https://dprint.dev");
        let reference = &references[1];
        assert_eq!(reference.range.start, 48);
        assert_eq!(reference.range.end, 75);
        assert_eq!(reference.name, "other");
        assert_eq!(reference.link, "https://github.com");
    }
}
