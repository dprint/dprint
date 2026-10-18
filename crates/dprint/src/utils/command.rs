use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandParseError {
  #[error("Found zero arguments.")]
  Empty,
  #[error("Unclosed single quote.")]
  UnclosedSingleQuote,
  #[error("Unclosed double quote.")]
  UnclosedDoubleQuote,
}

pub fn parse_command_line(input: &str) -> Result<Vec<String>, CommandParseError> {
  use CommandParseError::*;

  let mut args = Vec::new();
  let mut current = String::new();
  let mut in_single = false;
  let mut in_double = false;
  let mut chars = input.chars();

  while let Some(c) = chars.next() {
    match c {
      c if c.is_whitespace() && !in_single && !in_double => {
        if !current.is_empty() {
          args.push(std::mem::take(&mut current));
        }
      }
      '\'' if !in_double => {
        in_single = !in_single;
      }
      '"' if !in_single => {
        in_double = !in_double;
      }
      '\\' if !in_single => {
        if let Some(next) = chars.next() {
          current.push(next);
        } else {
          current.push('\\');
        }
      }

      _ => current.push(c),
    }
  }

  if in_single {
    return Err(UnclosedSingleQuote);
  }
  if in_double {
    return Err(UnclosedDoubleQuote);
  }

  if !current.is_empty() {
    args.push(current);
  }

  if args.is_empty() { Err(Empty) } else { Ok(args) }
}

#[cfg(test)]
mod tests {
  use super::CommandParseError;
  use super::parse_command_line;

  #[test]
  fn parses_simple_command() {
    let args = parse_command_line("vim").unwrap();
    assert_eq!(args, vec!["vim"]);
  }

  #[test]
  fn parses_command_with_arg() {
    let args = parse_command_line("vim -f").unwrap();
    assert_eq!(args, vec!["vim", "-f"]);
  }

  #[test]
  fn collapses_multiple_spaces() {
    let args = parse_command_line("  vim   -f   file.txt  ").unwrap();
    assert_eq!(args, vec!["vim", "-f", "file.txt"]);
  }

  #[test]
  fn parses_double_quoted_args() {
    let args = parse_command_line(r#""code" "--wait""#).unwrap();
    assert_eq!(args, vec!["code", "--wait"]);
  }

  #[test]
  fn parses_single_quoted_args_with_spaces() {
    let args = parse_command_line("nvim --cmd 'set number'").unwrap();
    assert_eq!(args, vec!["nvim", "--cmd", "set number"]);
  }

  #[test]
  fn parses_mixed_quotes() {
    let args = parse_command_line(r#"emacsclient -c -a "emacs -nw""#).unwrap();
    assert_eq!(args, vec!["emacsclient", "-c", "-a", "emacs -nw"]);
  }

  #[test]
  fn parses_path_with_escaped_space() {
    let args = parse_command_line(r#"/Applications/Sublime\ Text.app/Contents/SharedSupport/bin/subl -w"#).unwrap();
    assert_eq!(args, vec!["/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl", "-w",]);
  }

  #[test]
  fn backslash_escapes_next_char_outside_single_quotes() {
    let args = parse_command_line(r#"vim \"weird\"-name"#).unwrap();
    assert_eq!(args, vec!["vim", "\"weird\"-name"]);
  }

  #[test]
  fn backslash_is_literal_inside_single_quotes() {
    let args = parse_command_line(r#"'a\b c' other"#).unwrap();
    assert_eq!(args, vec!["a\\b c", "other"]);
  }

  #[test]
  fn empty_string_is_error() {
    let err = parse_command_line("").unwrap_err();
    assert!(matches!(err, CommandParseError::Empty));
  }

  #[test]
  fn whitespace_only_is_error() {
    let err = parse_command_line("   \t  ").unwrap_err();
    assert!(matches!(err, CommandParseError::Empty));
  }

  #[test]
  fn unclosed_single_quote_is_error() {
    let err = parse_command_line("vim 'unclosed").unwrap_err();
    assert!(matches!(err, CommandParseError::UnclosedSingleQuote));
  }

  #[test]
  fn unclosed_double_quote_is_error() {
    let err = parse_command_line(r#"vim "unclosed"#).unwrap_err();
    assert!(matches!(err, CommandParseError::UnclosedDoubleQuote));
  }
}
