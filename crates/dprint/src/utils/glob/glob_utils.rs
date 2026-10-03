use std::borrow::Cow;

use ignore::gitignore::GitignoreBuilder;

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

    // consume backslashes in pairs the same way `unescape_glob_text` does so
    // an escaped backslash doesn't escape the character after it
    // (ex. the `*` in `a\\*b` is a glob star)
    was_last_escape = c == '\\' && !was_last_escape;
  }
  false
}

/// Expands the brace groups in a pattern that span path components into
/// separate patterns (ex. `{.,src/**,worker}/*.js` -> `./*.js`, `src/**/*.js`
/// and `worker/*.js`).
///
/// The glob engine only understands a brace group within a single path
/// component (ex. `*.{ts,js}`), so those are left for it to handle.
pub fn expand_braces(pattern: &str) -> Vec<String> {
  if !pattern.contains('{') {
    return vec![pattern.to_string()];
  }
  let (negation, text) = if is_negated_glob(pattern) { ("!", &pattern[1..]) } else { ("", pattern) };
  let mut expansions = Vec::new();
  expand_braces_from(text, 0, &mut expansions);
  if expansions.len() == 1 && expansions[0] == text {
    return vec![pattern.to_string()];
  }

  // a pattern with a slash only matches relative to its base directory, so
  // keep it that way when an expansion ends up without one (ex. `{a/b,c}`)
  let is_anchored = text.trim_end_matches('/').contains('/');
  expansions
    .into_iter()
    .map(|mut expansion| {
      // a `.` alternative names the directory it's in (ex. `src/{.,sub}/*.js`)
      while let Some(index) = expansion.find("/./") {
        expansion.replace_range(index..index + 2, "");
      }
      if is_anchored && !expansion.trim_end_matches('/').contains('/') {
        format!("{}./{}", negation, expansion)
      } else {
        format!("{}{}", negation, expansion)
      }
    })
    .collect()
}

/// Whether a single path component pattern (ex. `dist`, `su*`, `[sd]ist`)
/// names the given directory.
///
/// Matching goes through the same gitignore engine the exclude matcher uses so
/// a wildcard means here what it means when the pattern is finally matched.
pub fn pattern_names_dir(pattern: &str, dir_name: &str) -> bool {
  if !is_pattern(pattern) {
    return unescape_glob_text(pattern) == dir_name;
  }
  // a component pattern can't match a name containing a separator
  if dir_name.contains('/') || dir_name.contains('\\') {
    return false;
  }
  let mut builder = GitignoreBuilder::new("");
  if builder.add_line(None, pattern).is_err() {
    return false;
  }
  match builder.build() {
    Ok(gitignore) => gitignore.matched(dir_name, true).is_ignore(),
    Err(_) => false,
  }
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

fn expand_braces_from(text: &str, start: usize, expansions: &mut Vec<String>) {
  let mut start = start;
  while let Some(group) = find_brace_group(text, start) {
    if group.spans_path_components(text) {
      for alternative in &group.alternatives {
        let expanded = format!("{}{}{}", &text[..group.open], alternative, &text[group.close + 1..]);
        // start at the alternative because it might contain a nested group
        expand_braces_from(&expanded, group.open, expansions);
      }
      return;
    }
    start = group.close + 1;
  }
  expansions.push(text.to_string());
}

struct BraceGroup<'a> {
  /// Index of the opening brace.
  open: usize,
  /// Index of the closing brace.
  close: usize,
  alternatives: Vec<&'a str>,
}

impl BraceGroup<'_> {
  fn spans_path_components(&self, text: &str) -> bool {
    let body = &text[self.open + 1..self.close];
    self.alternatives.len() > 1 && (body.contains('/') || body.split(['{', '}', ',']).any(|part| part == "."))
  }
}

/// Finds the first brace group at or after the start index, skipping over
/// escaped characters and character classes (ex. `[{]`).
fn find_brace_group(text: &str, start: usize) -> Option<BraceGroup<'_>> {
  let bytes = text.as_bytes();
  let mut index = start;
  while index < bytes.len() {
    match bytes[index] {
      b'\\' => index += 2,
      b'[' => index = skip_char_class(bytes, index),
      b'{' => return parse_brace_group(text, index),
      _ => index += 1,
    }
  }
  None
}

fn parse_brace_group(text: &str, open: usize) -> Option<BraceGroup<'_>> {
  let bytes = text.as_bytes();
  let mut alternatives = Vec::new();
  let mut alternative_start = open + 1;
  let mut depth = 1;
  let mut index = open + 1;
  while index < bytes.len() {
    match bytes[index] {
      b'\\' => {
        index += 2;
        continue;
      }
      b'[' => {
        index = skip_char_class(bytes, index);
        continue;
      }
      b'{' => depth += 1,
      b'}' => {
        depth -= 1;
        if depth == 0 {
          alternatives.push(&text[alternative_start..index]);
          return Some(BraceGroup {
            open,
            close: index,
            alternatives,
          });
        }
      }
      b',' if depth == 1 => {
        alternatives.push(&text[alternative_start..index]);
        alternative_start = index + 1;
      }
      _ => {}
    }
    index += 1;
  }
  None // unbalanced, so leave it for the glob engine to error on
}

/// Gets the index after the character class opening at the provided index,
/// or after the bracket when it doesn't open a character class.
fn skip_char_class(bytes: &[u8], open: usize) -> usize {
  let mut index = open + 1;
  if matches!(bytes.get(index), Some(b'!' | b'^')) {
    index += 1;
  }
  // a closing bracket at the start of a class is a literal (ex. `[]]`)
  if bytes.get(index) == Some(&b']') {
    index += 1;
  }
  while index < bytes.len() {
    if bytes[index] == b']' {
      return index + 1;
    }
    index += 1;
  }
  open + 1
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

    // an escaped backslash doesn't escape the character after it, so the
    // star in `a\\*b` is a glob star (matching `unescape_glob_text`)
    assert!(is_pattern("a\\\\*b"));
    assert_eq!(unescape_glob_text("a\\\\*b"), "a\\*b");
    // ...while `a\*b` is an escaped star and so not a pattern
    assert!(!is_pattern("a\\*b"));
    assert_eq!(unescape_glob_text("a\\*b"), "a*b");
  }

  #[test]
  fn should_expand_braces_spanning_path_components() {
    #[track_caller]
    fn run(pattern: &str, expected: &[&str]) {
      assert_eq!(expand_braces(pattern), expected);
    }

    run("{.,src/**,worker}/*.js", &["./*.js", "src/**/*.js", "worker/*.js"]);
    run("!{.,src/**}/*.js", &["!./*.js", "!src/**/*.js"]);
    run("src/{.,sub}/*.js", &["src/*.js", "src/sub/*.js"]);
    run("{a/b,c}/*.{ts,js}", &["a/b/*.{ts,js}", "c/*.{ts,js}"]);
    run("**/*.{ts,js}/{a/b,c}", &["**/*.{ts,js}/a/b", "**/*.{ts,js}/c"]);
    // nested
    run("{a,b/{c,d/e}}/f", &["a/f", "b/c/f", "b/d/e/f"]);
    run("{a/{b,c},d}", &["a/{b,c}", "./d"]);
    run("{a,{.,b/c}}/d", &["a/d", "./d", "b/c/d"]);
    // multiple groups
    run("{a/b,c}/{d/e,f}", &["a/b/d/e", "a/b/f", "c/d/e", "c/f"]);
    // stays relative to the base directory
    run("{a/b,c}", &["a/b", "./c"]);
    run("{,src/}*.js", &["./*.js", "src/*.js"]);
    run("./{a/b,c}", &["./a/b", "./c"]);

    // left for the glob engine
    run("**/*.ts", &["**/*.ts"]);
    run("**/*.{ts,js}", &["**/*.{ts,js}"]);
    run("{a,b}/*.ts", &["{a,b}/*.ts"]);
    run("{{myfile}}.yaml", &["{{myfile}}.yaml"]);
    run("{a/b}/c", &["{a/b}/c"]);
    run("a/[{]b/c,d}", &["a/[{]b/c,d}"]);
    run("a/[]{]b/c,d}", &["a/[]{]b/c,d}"]);
    run("a/\\{b/c,d\\}", &["a/\\{b/c,d\\}"]);
    run("{a/b,c", &["{a/b,c"]);
  }

  #[test]
  fn should_get_if_pattern_names_dir() {
    assert!(pattern_names_dir("dist", "dist"));
    assert!(!pattern_names_dir("dist", "other"));
    // wildcards mean what they mean when the pattern is finally matched
    assert!(pattern_names_dir("su*", "sub"));
    assert!(!pattern_names_dir("su*", "other"));
    assert!(pattern_names_dir("?ub", "sub"));
    assert!(pattern_names_dir("[sd]ist", "dist"));
    assert!(!pattern_names_dir("[sd]ist", "list"));
    assert!(pattern_names_dir("*", "anything"));
    assert!(pattern_names_dir("*.min.js", "a.min.js"));
    // both ways of escaping a glob character match the literal name
    assert!(pattern_names_dir("\\[id\\]", "[id]"));
    assert!(pattern_names_dir("[[]id[]]", "[id]"));
    // a single component pattern never matches across a separator
    assert!(!pattern_names_dir("dist", "dist/sub"));
    assert!(!pattern_names_dir("*", "dist/sub"));
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
