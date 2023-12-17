use crate::environment::CanonicalizedPathBuf;

pub fn is_negated_glob(pattern: &str) -> bool {
  let mut chars = pattern.chars();
  let first_char = chars.next();
  let second_char = chars.next();

  first_char == Some('!') && second_char != Some('(')
}

pub fn non_negated_glob(pattern: &str) -> &str {
  if is_negated_glob(pattern) {
    &pattern[1..]
  } else {
    pattern
  }
}

pub fn is_absolute_pattern(pattern: &str) -> bool {
  let pattern = if is_negated_glob(pattern) { &pattern[1..] } else { pattern };
  pattern.starts_with('/') || is_windows_absolute_pattern(pattern)
}

pub fn make_absolute(pattern: &str, base: &CanonicalizedPathBuf) -> String {
  if is_absolute_pattern(pattern) {
    pattern.to_string()
  } else {
    let base = base.to_string_lossy().to_string().replace('\\', "/");
    let is_negated = is_negated_glob(pattern);
    let pattern = if is_negated { &pattern[1..] } else { pattern };
    format!(
      "{}{}/{}",
      if is_negated { "!" } else { "" },
      base.trim_end_matches('/'),
      pattern.trim_start_matches("./")
    )
  }
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
  fn should_get_if_absolute_pattern() {
    assert_eq!(is_absolute_pattern("test.ts"), false);
    assert_eq!(is_absolute_pattern("!test.ts"), false);
    assert_eq!(is_absolute_pattern("/test.ts"), true);
    assert_eq!(is_absolute_pattern("!/test.ts"), true);
    assert_eq!(is_absolute_pattern("D:/test.ts"), true);
    assert_eq!(is_absolute_pattern("!D:/test.ts"), true);
  }

  #[test]
  fn test_make_absolute() {
    #[track_caller]
    fn run_test(pattern: &str, dir: &str, expected: &str) {
      assert_eq!(make_absolute(pattern, &CanonicalizedPathBuf::new_for_testing(dir)), expected);
    }

    run_test("./test", "/sub_dir", "/sub_dir/test");
    run_test("./test", "/sub_dir/", "/sub_dir/test");
    run_test("!./test/**/*", "/sub_dir/", "!/sub_dir/test/**/*");
    run_test("/test", "/sub_dir/", "/test");
    run_test("d:/test", "/sub_dir/", "d:/test");
    run_test("!d:/test", "/sub_dir/", "!d:/test");
  }
}
