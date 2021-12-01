use std::borrow::Cow;

pub fn to_absolute_glob(pattern: &str, dir: &str) -> String {
  // Adapted from https://github.com/dsherret/ts-morph/blob/0f8a77a9fa9d74e32f88f36992d527a2f059c6ac/packages/common/src/fileSystem/FileUtils.ts#L272

  // convert backslashes to forward slashes (don't worry about matching file names with back slashes)
  let mut pattern = pattern.replace("\\", "/");
  let dir = dir.replace("\\", "/");

  // check to see if glob is negated
  let is_negated = is_negated_glob(&pattern);
  if is_negated {
    pattern.drain(..1); // remove the leading "!"
  }

  // .gitignore: "If there is a separator at the beginning or middle (or both) of
  // the pattern, then the pattern is relative to the directory level of the particular
  // .gitignore file itself. Otherwise the pattern may also match at any level below the
  // .gitignore level."
  let is_relative = match pattern.find("/") {
    Some(index) => index != pattern.len() - 1, // not the end of the pattern
    None => false,
  };

  // trim starting ./ from glob patterns
  if pattern.starts_with("./") {
    pattern.drain(..2);
  }

  // when the glob pattern is only a . use an empty string
  if pattern == "." {
    pattern = String::new();
  }

  // store last character before glob is modified
  let suffix = pattern.chars().last();

  // make glob absolute
  if !is_absolute_pattern(&pattern) {
    if is_relative || pattern.starts_with("**/") || pattern.trim().is_empty() {
      pattern = glob_join(dir, pattern);
    } else {
      pattern = glob_join(dir, format!("**/{}", pattern));
    }
  }

  // if glob had a trailing `/`, re-add it back
  if suffix == Some('/') && !pattern.ends_with('/') {
    pattern.push('/');
  }

  if is_negated {
    format!("!{}", pattern)
  } else {
    pattern
  }
}

pub fn is_negated_glob(pattern: &str) -> bool {
  let mut chars = pattern.chars();
  let first_char = chars.next();
  let second_char = chars.next();

  return first_char == Some('!') && second_char != Some('(');
}

fn glob_join(dir: String, pattern: String) -> String {
  // strip trailing slash
  let dir = if dir.ends_with('/') {
    Cow::Borrowed(&dir[..dir.len() - 1])
  } else {
    Cow::Owned(dir)
  };
  // strip leading slash
  let pattern = if pattern.starts_with('/') {
    Cow::Borrowed(&pattern[1..])
  } else {
    Cow::Owned(pattern)
  };

  if pattern.len() == 0 {
    dir.into_owned()
  } else {
    format!("{}/{}", dir, pattern)
  }
}

pub fn is_absolute_pattern(pattern: &str) -> bool {
  let pattern = if is_negated_glob(pattern) { &pattern[1..] } else { &pattern };
  pattern.starts_with("/") || is_windows_absolute_pattern(pattern)
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
  fn should_get_absolute_globs() {
    assert_eq!(to_absolute_glob("**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test/"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("D:/**/*.ts", "/test/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "D:/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob(".", "D:\\test"), "D:/test");
    assert_eq!(to_absolute_glob("\\test\\asdf.ts", "D:\\test"), "/test/asdf.ts");
    assert_eq!(to_absolute_glob("!**/*.ts", "D:\\test"), "!D:/test/**/*.ts");
    assert_eq!(to_absolute_glob("///test/**/*.ts", "D:\\test"), "///test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "CD:\\"), "CD:/**/*.ts");

    assert_eq!(to_absolute_glob("./test.ts", "/test/"), "/test/test.ts");
    assert_eq!(to_absolute_glob("test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("*/test.ts", "/test/"), "/test/*/test.ts");
    assert_eq!(to_absolute_glob("*test.ts", "/test/"), "/test/**/*test.ts");
    assert_eq!(to_absolute_glob("**/test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("/test.ts", "/test/"), "/test.ts");
    assert_eq!(to_absolute_glob("test/", "/test/"), "/test/**/test/");

    assert_eq!(to_absolute_glob("!./test.ts", "/test/"), "!/test/test.ts");
    assert_eq!(to_absolute_glob("!test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!*/test.ts", "/test/"), "!/test/*/test.ts");
    assert_eq!(to_absolute_glob("!*test.ts", "/test/"), "!/test/**/*test.ts");
    assert_eq!(to_absolute_glob("!**/test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!/test.ts", "/test/"), "!/test.ts");
    assert_eq!(to_absolute_glob("!test/", "/test/"), "!/test/**/test/");
    // has a slash in the middle, so it's relative
    assert_eq!(to_absolute_glob("test/test.ts", "/test/"), "/test/test/test.ts");
  }
}
