use dissimilar::*;
use colored::Colorize;

use dprint_core::types::ErrBox;

// TODO: This file needs improvement as it is kind of buggy, but
// does the job for now.

/// Gets a string showing the difference between two strings.
/// Note: This returns a Result because this funciton has been unstable.
pub fn get_difference(text1: &str, text2: &str) -> Result<String, ErrBox> {
    debug_assert!(text1 != text2);

    // normalize newlines
    let text1 = text1.replace("\r\n", "\n");
    let text2 = text2.replace("\r\n", "\n");

    if text1 == text2 {
        return Ok(String::from(" | Text differed by line endings."));
    }

    let grouped_changes = get_grouped_changes(&text1, &text2);
    let mut text = String::new();

    for (i, grouped_change) in grouped_changes.into_iter().enumerate() {
        if i > 0 {
            text.push_str("\n...\n");
        }

        let max_line_num_width = grouped_change.end_line_number.to_string().chars().count();
        text.push_str(&format!("{:width$}| ", grouped_change.start_line_number, width = max_line_num_width));
        text.push_str(&annotate_whitespace(get_line_start_text(&text1, grouped_change.start_index)?));
        let mut last_index = grouped_change.start_index;

        for change in grouped_change.changes {
            for (i, line) in text1[last_index..change.start_index()].split('\n').enumerate() {
                if i > 0 {
                    text.push_str(&format!("\n{}| ", " ".repeat(max_line_num_width)));
                }

                text.push_str(&annotate_whitespace(line));
            }

            last_index = change.end_index();

            match change {
                Change::Addition(addition) => {
                    for (i, line) in addition.new_text.split('\n').enumerate() {
                        if i > 0 {
                            text.push_str("\n");
                            text.push_str(&get_addition_text(&format!("{}| ", " ".repeat(max_line_num_width))));
                        }

                        if !line.is_empty() {
                            text.push_str(&get_addition_text(&annotate_whitespace(line)));
                        }
                    }
                },
                Change::Removal(removal) => {
                    for (i, line) in removal.removed_text.split('\n').enumerate() {
                        if i > 0 {
                            text.push_str("\n");
                            let line_text = format!("{}| ", " ".repeat(max_line_num_width));
                            if line.is_empty() {
                                text.push_str(&get_removal_text(&line_text));
                            } else {
                                text.push_str(&line_text);
                            }
                        }

                        if !line.is_empty() {
                            text.push_str(&get_removal_text(&annotate_whitespace(line)));
                        }
                    }
                }
            }
        }

        text.push_str(&annotate_whitespace(&get_line_end_text(&text1, grouped_change.end_index)));
    }

    Ok(text)
}

fn get_line_start_text<'a>(text: &'a str, index: usize) -> Result<&'a str, ErrBox> {
    let new_line_byte = '\n' as u8;
    let text_bytes = text.as_bytes();
    let mut start_index = 0;
    let mut length = 0;

    if index > text.len() {
        // this should never happen
        return err!("The byte index was {}, but the text byte length is {}.", index, text.len());
    }

    for i in (0..index).rev() {
        if text_bytes[i] == new_line_byte || length > 50 {
            start_index = i + 1;
            break;
        }
        length += 1
    }

    Ok(&text[start_index..index])
}

fn get_line_end_text<'a>(text: &'a str, index: usize) -> &'a str {
    let new_line_byte = '\n' as u8;
    let text_bytes = text.as_bytes();
    let mut end_index = index;
    let mut length = 0;

    for i in index..text.len() {
        if text_bytes[i] == new_line_byte || length > 50 {
            end_index = i;
            break;
        }
        length += 1;
    }

    &text[index..end_index]
}

#[derive(Debug)]
struct GroupedChange<'a> {
    start_index: usize,
    end_index: usize,
    start_line_number: usize,
    end_line_number: usize,
    changes: Vec<Change<'a>>,
}

fn get_grouped_changes<'a>(text1: &'a str, text2: &'a str) -> Vec<GroupedChange<'a>> {
    let changes = get_changes(text1, text2);
    let mut grouped_changes: Vec<GroupedChange<'a>> = Vec::new();

    for change in changes {
        if let Some(grouped_change) = grouped_changes.last_mut() {
            // keeps changes together if they are only separated by a single line
            const GROUPED_LINE_COUNT: usize = 2;
            let should_group = change.start_line_number() < GROUPED_LINE_COUNT // prevent overflow
                || grouped_change.end_line_number >= change.start_line_number() - GROUPED_LINE_COUNT;
            if should_group {
                grouped_change.end_index = change.end_index();
                grouped_change.end_line_number = change.end_line_number();
                grouped_change.changes.push(change);
                continue;
            }
        }

        grouped_changes.push(GroupedChange {
            start_index: change.start_index(),
            end_index: change.end_index(),
            start_line_number: change.start_line_number(),
            end_line_number: change.end_line_number(),
            changes: vec![change],
        })
    }

    grouped_changes
}

#[derive(Debug)]
enum Change<'a> {
    Addition(Addition<'a>),
    Removal(Removal<'a>),
}

impl<'a> Change<'a> {
    /// Gets the start index in the original string.
    fn start_index(&self) -> usize {
        match self {
            Change::Addition(addition) => addition.insert_index,
            Change::Removal(removal) => removal.start_index,
        }
    }

    /// Gets the end index in the original string.
    fn end_index(&self) -> usize {
        match self {
            Change::Addition(addition) => addition.insert_index,
            Change::Removal(removal) => removal.end_index,
        }
    }

    /// Gets the start line number in the original string.
    fn start_line_number(&self) -> usize {
        match self {
            Change::Addition(addition) => addition.insert_line_number,
            Change::Removal(removal) => removal.start_line_number,
        }
    }

    /// Gets the end line number in the original string.
    fn end_line_number(&self) -> usize {
        match self {
            Change::Addition(addition) => addition.insert_line_number,
            Change::Removal(removal) => removal.end_line_number,
        }
    }
}

#[derive(Debug)]
struct Addition<'a> {
    new_text: &'a str,
    insert_index: usize,
    insert_line_number: usize,
}

#[derive(Debug)]
struct Removal<'a> {
    removed_text: &'a str,
    start_index: usize,
    end_index: usize,
    start_line_number: usize,
    end_line_number: usize,
}

fn get_changes<'a>(text1: &'a str, text2: &'a str) -> Vec<Change<'a>> {
    let chunks = get_pre_processed_chunks(text1, text2);
    let mut changes: Vec<Change<'a>> = Vec::new();

    let mut line_number = 1;
    let mut byte_index = 0;

    for i in 0..chunks.len() {
        let chunk = chunks[i];
        match chunk {
            Chunk::Insert(inserted_text) => {
                changes.push(Change::Addition(Addition {
                    new_text: inserted_text,
                    insert_index: byte_index,
                    insert_line_number: line_number,
                }));
            },
            Chunk::Delete(deleted_text) => {
                let line_count = deleted_text.split('\n').count() - 1;

                changes.push(Change::Removal(Removal {
                    removed_text: deleted_text,
                    start_index: byte_index,
                    end_index: byte_index + deleted_text.len(),
                    start_line_number: line_number,
                    end_line_number: line_number + line_count,
                }));

                byte_index += deleted_text.len();
                line_number += line_count;
            },
            Chunk::Equal(equal_text) => {
                byte_index += equal_text.len();
                line_number += equal_text.split('\n').count() - 1;
            },
        }
    }

    changes
}

fn get_pre_processed_chunks<'a>(text1: &'a str, text2: &'a str) -> Vec<dissimilar::Chunk<'a>> {
    // What we want to do here is take a collection of chunks like this:
    //   [Equal("class Test"), Delete("\n"), Insert(" "), Equal("{\n"), Delete("\n"), Equal("}"), Insert("\n")]
    // And transform them so the parts that proceed the newline delete end up as inserts on the previous line and deletes on the next:
    //   [Equal("class Test"), Insert(" "), Insert("{"), Equal("\n"), Delete("{"), Delete("\n"), Delete("\n"), Equal("}"), Insert("\n")]
    // Note: It would probably be nice to group like chunks together here... for the future...
    let chunks = dissimilar::diff(text1, text2);

    let mut final_chunks = Vec::new();
    let mut i = 0;
    while i < chunks.len() {
        let chunk = chunks[i];

        let mut was_deleted_with_newline = false;
        if let Chunk::Delete(delete_text) = chunk {
            if let Some(Chunk::Equal(last_change)) = final_chunks.last() {
                was_deleted_with_newline = delete_text.find('\n').is_some() && !last_change.ends_with('\n');
            }
        }

        if was_deleted_with_newline {
            let add_delete_at_end = if let Some(Chunk::Insert(insert_text)) = chunks.get(i + 1) {
                // Do not add a delete newline at the end if the next chunk is inserting a newline.
                // TBH: This doesn't seem exactly right and I did this to fix a bug.
                !insert_text.contains('\n')
            } else {
                true
            };

            let delete_text = if let Chunk::Delete(delete_text) = chunk { delete_text } else { unreachable!() };

            // push delete text previous line
            let delete_text_new_line_index = delete_text.find('\n').unwrap();
            if delete_text_new_line_index > 0 {
                final_chunks.push(dissimilar::Chunk::Delete(&delete_text[0..delete_text_new_line_index]));
            }

            // push delete text next line for next line chunks
            let mut next_line_chunks = Vec::new();
            if delete_text_new_line_index + 1 < delete_text.len() {
                next_line_chunks.push(dissimilar::Chunk::Delete(&delete_text[delete_text_new_line_index + 1..]));
            }

            i += 1;

            // move the next equal or insert chunks to be inserts on the previous line and deletes on the current
            while i < chunks.len() {
                let chunk = chunks[i];
                match chunk {
                    Chunk::Equal(equal_text) => {
                        let new_line_index = equal_text.find('\n');
                        if let Some(new_line_index) = new_line_index {
                            // for previous line
                            final_chunks.push(dissimilar::Chunk::Insert(&equal_text[0..new_line_index]));

                            // for next line
                            next_line_chunks.push(dissimilar::Chunk::Delete(&equal_text[0..new_line_index]));

                            // for remainder of next line equal text
                            let remainder_equal_text = &equal_text[new_line_index + 1..];
                            if remainder_equal_text.len() > 0 {
                                next_line_chunks.push(dissimilar::Chunk::Equal(remainder_equal_text));
                            }
                            break;
                        } else {
                            final_chunks.push(dissimilar::Chunk::Insert(equal_text));
                            next_line_chunks.push(dissimilar::Chunk::Delete(equal_text));
                        }
                    },
                    Chunk::Insert(insert_text) => {
                        let new_line_index = insert_text.find('\n');
                        if let Some(new_line_index) = new_line_index {
                            // for previous line
                            final_chunks.push(dissimilar::Chunk::Insert(&insert_text[0..new_line_index]));
                            // for next line
                            let remainder_text = &insert_text[new_line_index + 1..];
                            if remainder_text.len() > 0 {
                                next_line_chunks.push(dissimilar::Chunk::Insert(remainder_text));
                            }
                            break;
                        } else {
                            final_chunks.push(dissimilar::Chunk::Insert(insert_text));
                        }
                    }
                    Chunk::Delete(delete_text) => {
                        next_line_chunks.push(dissimilar::Chunk::Delete(delete_text));
                    }
                }
                i += 1;
            }

            final_chunks.push(dissimilar::Chunk::Equal(&delete_text[delete_text_new_line_index..delete_text_new_line_index+1]));
            final_chunks.extend(next_line_chunks);
            if add_delete_at_end {
                final_chunks.push(dissimilar::Chunk::Delete(&delete_text[delete_text_new_line_index..delete_text_new_line_index+1]));
            }
        } else {
            final_chunks.push(chunk);
        }

        i += 1;
    }

    final_chunks
}

fn get_addition_text(text: &str) -> String {
    text.white().on_green().to_string()
}

fn get_removal_text(text: &str) -> String {
    let text = text.replace("\t", "\u{21E5}");
    text.white().on_red().to_string()
}

fn annotate_whitespace(text: &str) -> String {
    text.replace("\t", "\u{2192}")
        .replace(" ", "\u{00B7}")
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use super::*;

    #[test]
    fn it_should_get_when_differs_by_line_endings() {
        assert_eq!(get_difference("test\r\n", "test\n").unwrap(), " | Text differed by line endings.");
    }

    #[test]
    fn it_should_get_difference_on_one_line() {
        assert_eq!(get_difference("test1\n", "test2\n").unwrap(), format!("1| test{}{}", get_removal_text("1"), get_addition_text("2")));
    }

    #[test]
    fn it_should_show_the_addition_of_last_line() {
        assert_eq!(
            get_difference("testing\ntesting", "testing\ntesting\n").unwrap(),
            format!(
                "{}\n{}",
                "2| testing",
                get_addition_text(&format!(" | "))
            )
        );
    }

    #[test]
    fn it_should_get_difference_for_removed_line() {
        assert_eq!(
            get_difference("class Test\n{\n\n}", "class Test {\n}\n").unwrap(),
            format!(
                "{}\n{}\n{}\n{}\n{}",
                format!("1| class\u{00B7}Test{}{}", get_addition_text("\u{00B7}"), get_addition_text("{")),
                format!(" | {}", get_removal_text("{")),
                get_removal_text(" | "),
                format!("{}{}", get_removal_text(" | "), "}"),
                get_addition_text(" | "),
            )
        );
    }

    #[test]
    fn it_should_show_multiple_removals_on_different_lines() {
        assert_eq!(
            get_difference("let t ;\n\n\nlet u ;\n", "let t;\n\n\nlet u;\n").unwrap(),
            format!(
                "{}\n...\n{}",
                format!("1| let\u{00B7}t{};", get_removal_text("\u{00B7}")),
                format!("4| let\u{00B7}u{};", get_removal_text("\u{00B7}")),
            )
        );
    }

    #[test]
    fn it_should_keep_grouped_when_changes_only_separated_by_one_line() {
        assert_eq!(
            get_difference("let t ;\ntest;\nlet u ;\n", "let t;\ntest;\nlet u;\n").unwrap(),
            format!(
                "{}\n{}\n{}",
                format!("1| let\u{00B7}t{};", get_removal_text("\u{00B7}")),
                " | test;",
                format!(" | let\u{00B7}u{};", get_removal_text("\u{00B7}")),
            )
        );
    }

    #[test]
    fn it_should_annotate_whitespace_end_line_text() {
        assert_eq!(
            get_difference("t t t\n", "tt t\n").unwrap(),
            format!(
                "1| t{}t\u{00B7}t",
                get_removal_text("\u{00B7}")
            )
        );
    }

    #[test]
    fn it_should_handle_replacements() {
        assert_eq!(
            get_difference("use::asdf\nuse::test", "use::other\nsomething").unwrap(),
            format!(
                "1| use::{}{}\n | {}{}",
                get_removal_text("asdf"),
                get_addition_text("other"),
                get_removal_text("use::test"),
                get_addition_text("something")
            )
        );
    }
}
