use std::borrow::Cow;

pub fn is_negated_glob(pattern: &str) -> bool {
  let mut chars = pattern.chars();
  let first_char = chars.next();
  let second_char = chars.next();

  first_char == Some('!') && second_char != Some('(')
}

pub fn non_negated_glob(pattern: &str) -> &str {
  if is_negated_glob(pattern) { &pattern[1..] } else { pattern }
}

pub fn is_pattern(pattern: &str) -> bool {
  if pattern.starts_with('!') {
    return true;
  }

  let mut was_last_escape = false;
  for c in pattern.chars() {
    if !was_last_escape && matches!(c, '*' | '{' | '?' | '[') {
      return true;
    }

    was_last_escape = matches!(c, '\\');
  }
  false
}

/// Escapes glob metacharacters so the text matches literally
/// (ex. `routes/[id].svelte` -> `routes/\[id\].svelte`).
pub fn escape_glob_text(text: &str) -> String {
  let mut result = String::with_capacity(text.len());
  for c in text.chars() {
    if matches!(c, '\\' | '*' | '{' | '}' | '?' | '[' | ']' | '!') {
      result.push('\\');
    }
    result.push(c);
  }
  result
}

/// Escapes glob metacharacters using character classes (ex. `[` -> `[[]`) so
/// the result contains no backslashes and survives CLI pattern processing,
/// which converts backslashes to forward slashes.
pub fn escape_glob_text_for_cli(text: &str) -> String {
  let mut result = String::with_capacity(text.len());
  for c in text.chars() {
    match c {
      '[' | ']' | '{' | '}' | '*' | '?' => {
        result.push('[');
        result.push(c);
        result.push(']');
      }
      _ => result.push(c),
    }
  }
  result
}

/// Removes glob escapes (ex. `routes/\[id\].svelte` -> `routes/[id].svelte`).
pub fn unescape_glob_text(text: &str) -> Cow<'_, str> {
  if !text.contains('\\') {
    return Cow::Borrowed(text);
  }
  let mut result = String::with_capacity(text.len());
  let mut chars = text.chars();
  while let Some(c) = chars.next() {
    if c == '\\' {
      match chars.next() {
        Some(next) => result.push(next),
        None => result.push(c),
      }
    } else {
      result.push(c);
    }
  }
  Cow::Owned(result)
}

pub fn is_absolute_pattern(pattern: &str) -> bool {
  let pattern = if is_negated_glob(pattern) { &pattern[1..] } else { pattern };
  pattern.starts_with('/') || is_windows_absolute_pattern(pattern)
}

fn is_windows_absolute_pattern(pattern: &str) -> bool {
  // ex. D:/
  let mut chars = pattern.chars();

  // ensure the first character is alphabetic
  let next_char = chars.next();
  if next_char.is_none() || !next_char.unwrap().is_ascii_alphabetic() {
    return false;
  }

  // skip over the remaining alphabetic characters
  let mut next_char = chars.next();
  while next_char.is_some() && next_char.unwrap().is_ascii_alphabetic() {
    next_char = chars.next();
  }

  // ensure colon
  if next_char != Some(':') {
    return false;
  }

  // now check for the last slash
  let next_char = chars.next();
  matches!(next_char, Some('/'))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn should_escape_and_unescape_glob_text() {
    assert_eq!(escape_glob_text("routes/[id].svelte"), "routes/\\[id\\].svelte");
    assert_eq!(escape_glob_text("{{myfile}}.yaml"), "\\{\\{myfile\\}\\}.yaml");
    assert_eq!(escape_glob_text("a*b?c!d\\e"), "a\\*b\\?c\\!d\\\\e");
    assert_eq!(escape_glob_text("plain/file.txt"), "plain/file.txt");

    assert_eq!(escape_glob_text_for_cli("/[app]/dir"), "/[[]app[]]/dir");
    assert_eq!(escape_glob_text_for_cli("/plain/dir"), "/plain/dir");

    assert_eq!(unescape_glob_text("routes/\\[id\\].svelte"), "routes/[id].svelte");
    assert_eq!(unescape_glob_text("plain/file.txt"), "plain/file.txt");
    assert_eq!(unescape_glob_text("a\\\\b"), "a\\b");
    // a trailing lone backslash stays as-is
    assert_eq!(unescape_glob_text("a\\"), "a\\");

    // escaped text is not considered a pattern and round trips
    assert!(is_pattern("routes/[id].svelte"));
    assert!(!is_pattern(&escape_glob_text("routes/[id].svelte")));
    assert_eq!(unescape_glob_text(&escape_glob_text("a*b?c!d\\e[]{}")), "a*b?c!d\\e[]{}");
  }

  #[test]
  fn should_get_if_absolute_pattern() {
    assert_eq!(is_absolute_pattern("test.ts"), false);
    assert_eq!(is_absolute_pattern("!test.ts"), false);
    assert_eq!(is_absolute_pattern("/test.ts"), true);
    assert_eq!(is_absolute_pattern("!/test.ts"), true);
    assert_eq!(is_absolute_pattern("D:/test.ts"), true);
    assert_eq!(is_absolute_pattern("!D:/test.ts"), true);
  }
}
