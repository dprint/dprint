use std::str::FromStr;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Result;
use anyhow::bail;

/// How old a package version must be before dprint will select it, as given by
/// `--minimum-dependency-age` or an `.npmrc`'s `min-release-age`.
///
/// The accepted forms match Deno's so a value can move between the two:
/// an ISO-8601 duration (`P3D`, `PT72H`), a bare integer of minutes (`1440`),
/// an absolute date (`2026-01-15`) or RFC3339 timestamp, and `0` to disable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimumDependencyAge {
  /// No age requirement — every published version is selectable.
  Disabled,
  /// How old a version must be, relative to now.
  Age(Duration),
  /// A fixed point in time. Nothing published after it is selectable.
  Cutoff(SystemTime),
}

impl MinimumDependencyAge {
  /// A whole number of days, as `.npmrc`'s `min-release-age` expresses it.
  pub fn from_days(days: u64) -> Self {
    match days {
      0 => MinimumDependencyAge::Disabled,
      days => MinimumDependencyAge::Age(Duration::from_secs(days * 60 * 60 * 24)),
    }
  }

  /// The instant that publish times are compared against, given the current
  /// time. `None` when no age requirement is in effect.
  pub fn cutoff_time(&self, now: SystemTime) -> Option<SystemTime> {
    match self {
      MinimumDependencyAge::Disabled => None,
      // an age longer than the unix epoch saturates rather than panicking,
      // which rules out every version instead of crashing
      MinimumDependencyAge::Age(age) => Some(now.checked_sub(*age).unwrap_or(SystemTime::UNIX_EPOCH)),
      MinimumDependencyAge::Cutoff(time) => Some(*time),
    }
  }
}

/// The instant npm publish times are compared against once a
/// [`MinimumDependencyAge`] has been resolved against the current time: a
/// version published after it is too new to select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyAgeCutoff {
  time: SystemTime,
  /// Where the requirement came from, for error and warning messages
  /// (ex. `P3D` or `min-release-age=3` in an .npmrc).
  description: String,
}

impl DependencyAgeCutoff {
  /// `None` when the age doesn't restrict anything, so callers can treat "not
  /// configured" and "explicitly disabled" the same way.
  pub fn new(age: &MinimumDependencyAge, description: impl Into<String>, now: SystemTime) -> Option<Self> {
    Some(DependencyAgeCutoff {
      time: age.cutoff_time(now)?,
      description: description.into(),
    })
  }

  pub fn is_too_new(&self, published: SystemTime) -> bool {
    published > self.time
  }

  pub fn description(&self) -> &str {
    &self.description
  }
}

impl FromStr for MinimumDependencyAge {
  type Err = anyhow::Error;

  fn from_str(text: &str) -> Result<Self> {
    let text = text.trim();
    if text.is_empty() {
      bail!("{}", INVALID_VALUE_MESSAGE);
    }
    // a bare integer is a count of minutes, the same as npm's --before-style config
    if text.bytes().all(|b| b.is_ascii_digit()) {
      let minutes = text
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("Minimum dependency age in minutes is too large: '{}'", text))?;
      return Ok(match minutes {
        0 => MinimumDependencyAge::Disabled,
        minutes => MinimumDependencyAge::Age(Duration::from_secs(minutes * 60)),
      });
    }
    if text.starts_with('P') || text.starts_with('p') {
      let duration = parse_iso8601_duration(text)?;
      return Ok(if duration.is_zero() {
        MinimumDependencyAge::Disabled
      } else {
        MinimumDependencyAge::Age(duration)
      });
    }
    match parse_rfc3339(text) {
      Some(time) => Ok(MinimumDependencyAge::Cutoff(time)),
      None => bail!("{}", INVALID_VALUE_MESSAGE),
    }
  }
}

const INVALID_VALUE_MESSAGE: &str = concat!(
  "Invalid minimum dependency age. Expected an ISO-8601 duration (ex. 'P3D' or 'PT72H'), ",
  "a number of minutes (ex. '1440'), a date (ex. '2026-01-15') or RFC3339 timestamp, or '0' to disable.",
);

/// Parses an RFC3339 timestamp (as npm publishes in a packument's `time`
/// property) or a plain `YYYY-MM-DD` date, which is taken as midnight UTC.
///
/// Returns `None` for anything that isn't one of those, so a registry serving
/// a timestamp dprint can't read is treated as one that reports no date at all
/// rather than failing the command.
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
  let (date, rest) = text.split_once(['T', 't']).unwrap_or((text, ""));
  let mut date_parts = date.splitn(3, '-');
  let year = date_parts.next()?.parse::<i64>().ok()?;
  let month = date_parts.next()?.parse::<u32>().ok()?;
  let day = date_parts.next()?.parse::<u32>().ok()?;
  if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
    return None;
  }

  let mut seconds = days_from_civil(year, month, day).checked_mul(60 * 60 * 24)?;
  if !rest.is_empty() {
    let (time, offset_seconds) = split_utc_offset(rest)?;
    // the fractional part only ever moves the timestamp by under a second,
    // which can't change which side of a cutoff a version falls on
    let time = time.split_once('.').map(|(time, _fraction)| time).unwrap_or(time);
    let mut time_parts = time.splitn(3, ':');
    let hour = time_parts.next()?.parse::<i64>().ok()?;
    let minute = time_parts.next()?.parse::<i64>().ok()?;
    // seconds are optional so a `2026-01-15T00:00Z` style timestamp still reads
    let second = match time_parts.next() {
      Some(second) => second.parse::<i64>().ok()?,
      None => 0,
    };
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
      return None;
    }
    seconds = seconds.checked_add(hour * 60 * 60 + minute * 60 + second)?;
    seconds = seconds.checked_sub(offset_seconds)?;
  }

  if seconds >= 0 {
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
  } else {
    SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
  }
}

/// Splits a timestamp's time portion from its UTC offset, returning the offset
/// in seconds. A timestamp with no offset is read as UTC — npm always publishes
/// `Z`, and guessing at a local time zone would be worse than assuming UTC.
fn split_utc_offset(time: &str) -> Option<(&str, i64)> {
  if let Some(time) = time.strip_suffix(['Z', 'z']) {
    return Some((time, 0));
  }
  let sign_index = time.rfind(['+', '-']);
  let Some(sign_index) = sign_index else {
    return Some((time, 0));
  };
  let (time, offset) = time.split_at(sign_index);
  let (sign, offset) = offset.split_at(1);
  let (hours, minutes) = match offset.split_once(':') {
    Some((hours, minutes)) => (hours, minutes),
    // an offset may be written without its separator (ex. `+0200`)
    None if offset.len() == 4 => offset.split_at(2),
    None => (offset, "0"),
  };
  let hours = hours.parse::<i64>().ok()?;
  let minutes = minutes.parse::<i64>().ok()?;
  if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
    return None;
  }
  let seconds = hours * 60 * 60 + minutes * 60;
  Some((time, if sign == "-" { -seconds } else { seconds }))
}

/// Parses the subset of ISO-8601 durations that has a fixed length: weeks,
/// days, hours, minutes and seconds.
///
/// Years and months are rejected because neither has a fixed number of days,
/// so `P1M` can't be turned into an age without a calendar the rest of this
/// module doesn't need.
fn parse_iso8601_duration(text: &str) -> Result<Duration> {
  let body = &text[1..]; // the leading `P`, already matched by the caller
  let (date_part, time_part) = match body.split_once(['T', 't']) {
    Some((date_part, time_part)) => (date_part, Some(time_part)),
    None => (body, None),
  };
  let mut seconds: u64 = 0;
  let mut saw_component = false;
  let mut parse_components = |part: &str, is_time: bool| -> Result<()> {
    let mut digits = String::new();
    for c in part.chars() {
      if c.is_ascii_digit() {
        digits.push(c);
        continue;
      }
      let value = digits
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("Invalid minimum dependency age '{}': expected a number before '{}'.", text, c))?;
      digits.clear();
      let multiplier = match (c.to_ascii_uppercase(), is_time) {
        ('W', false) => 60 * 60 * 24 * 7,
        ('D', false) => 60 * 60 * 24,
        ('H', true) => 60 * 60,
        ('M', true) => 60,
        ('S', true) => 1,
        ('Y', false) | ('M', false) => bail!(
          "Invalid minimum dependency age '{}': years and months don't have a fixed length. Use days instead (ex. 'P30D').",
          text
        ),
        _ => bail!("Invalid minimum dependency age '{}': unexpected '{}'.", text, c),
      };
      seconds = seconds
        .checked_add(value.saturating_mul(multiplier))
        .ok_or_else(|| anyhow::anyhow!("Minimum dependency age is too large: '{}'", text))?;
      saw_component = true;
    }
    if !digits.is_empty() {
      bail!("Invalid minimum dependency age '{}': '{}' is missing a unit.", text, digits);
    }
    Ok(())
  };
  parse_components(date_part, false)?;
  if let Some(time_part) = time_part {
    parse_components(time_part, true)?;
  }
  if !saw_component {
    bail!("{}", INVALID_VALUE_MESSAGE);
  }
  Ok(Duration::from_secs(seconds))
}

/// Days since the unix epoch for a civil (proleptic Gregorian) date, using
/// Howard Hinnant's `days_from_civil` algorithm.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
  let year = if month <= 2 { year - 1 } else { year };
  let era = if year >= 0 { year } else { year - 399 } / 400;
  let year_of_era = year - era * 400; // [0, 399]
  let month = month as i64;
  let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day as i64 - 1;
  let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
  era * 146097 + day_of_era - 719468
}

fn days_in_month(year: i64, month: u32) -> u32 {
  match month {
    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
    4 | 6 | 9 | 11 => 30,
    2 if is_leap_year(year) => 29,
    2 => 28,
    _ => 0,
  }
}

fn is_leap_year(year: i64) -> bool {
  year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod test {
  use super::*;
  use pretty_assertions::assert_eq;

  fn parse(text: &str) -> Result<MinimumDependencyAge> {
    MinimumDependencyAge::from_str(text)
  }

  fn age_secs(text: &str) -> u64 {
    match parse(text).unwrap() {
      MinimumDependencyAge::Age(duration) => duration.as_secs(),
      other => panic!("expected an age, got {:?}", other),
    }
  }

  #[test]
  fn parses_iso8601_durations() {
    assert_eq!(age_secs("P3D"), 3 * 86400);
    assert_eq!(age_secs("PT72H"), 72 * 3600);
    assert_eq!(age_secs("P1W"), 7 * 86400);
    assert_eq!(age_secs("P1DT12H30M15S"), 86400 + 12 * 3600 + 30 * 60 + 15);
    assert_eq!(age_secs("PT30M"), 30 * 60);
    // the date part's `M` is months while the time part's is minutes
    assert_eq!(age_secs("p2d"), 2 * 86400);
  }

  #[test]
  fn parses_minutes() {
    assert_eq!(age_secs("1440"), 1440 * 60);
    assert_eq!(age_secs("1"), 60);
  }

  #[test]
  fn parses_zero_as_disabled() {
    assert_eq!(parse("0").unwrap(), MinimumDependencyAge::Disabled);
    assert_eq!(parse("PT0S").unwrap(), MinimumDependencyAge::Disabled);
    assert_eq!(parse("P0D").unwrap(), MinimumDependencyAge::Disabled);
  }

  #[test]
  fn parses_absolute_dates_and_timestamps() {
    let date = parse("2026-01-15").unwrap();
    assert_eq!(date, MinimumDependencyAge::Cutoff(unix(1768435200)));
    let timestamp = parse("2026-01-15T12:00:00Z").unwrap();
    assert_eq!(timestamp, MinimumDependencyAge::Cutoff(unix(1768435200 + 12 * 3600)));
  }

  #[test]
  fn rejects_invalid_values() {
    for text in ["", "abc", "P", "P1Y", "P1M", "P3", "PT", "3days", "2026-13-01", "2026-02-30"] {
      assert!(parse(text).is_err(), "expected '{}' to be rejected", text);
    }
    assert!(parse("P1Y").unwrap_err().to_string().contains("fixed length"));
  }

  #[test]
  fn from_days_matches_npmrc_semantics() {
    assert_eq!(MinimumDependencyAge::from_days(0), MinimumDependencyAge::Disabled);
    assert_eq!(MinimumDependencyAge::from_days(3), MinimumDependencyAge::Age(Duration::from_secs(3 * 86400)));
  }

  #[test]
  fn computes_the_cutoff_time() {
    let now = unix(1_000_000);
    assert_eq!(MinimumDependencyAge::Disabled.cutoff_time(now), None);
    assert_eq!(MinimumDependencyAge::Age(Duration::from_secs(400)).cutoff_time(now), Some(unix(999_600)));
    assert_eq!(MinimumDependencyAge::Cutoff(unix(5)).cutoff_time(now), Some(unix(5)));
    // an age reaching past the epoch saturates instead of panicking
    assert_eq!(
      MinimumDependencyAge::Age(Duration::from_secs(u64::MAX)).cutoff_time(now),
      Some(SystemTime::UNIX_EPOCH)
    );
  }

  #[test]
  fn parses_rfc3339_timestamps() {
    assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(unix(0)));
    assert_eq!(parse_rfc3339("2024-05-01T12:34:56.789Z"), Some(unix(1714566896)));
    // npm always publishes `Z`, but an offset timestamp still reads correctly
    assert_eq!(parse_rfc3339("2024-05-01T14:34:56+02:00"), Some(unix(1714566896)));
    assert_eq!(parse_rfc3339("2024-05-01T14:34:56+0200"), Some(unix(1714566896)));
    assert_eq!(parse_rfc3339("2024-05-01T10:34:56-02:00"), Some(unix(1714566896)));
    // a missing offset is read as utc, and seconds are optional
    assert_eq!(parse_rfc3339("2024-05-01T12:34:56"), Some(unix(1714566896)));
    assert_eq!(parse_rfc3339("2024-05-01T12:34Z"), Some(unix(1714566840)));
    assert_eq!(parse_rfc3339("2024-05-01"), Some(unix(1714521600)));
    // leap years
    assert_eq!(parse_rfc3339("2024-02-29"), Some(unix(1709164800)));
    assert_eq!(parse_rfc3339("2023-02-29"), None);
    // a timestamp we can't read reports no date rather than erroring
    for text in ["", "not a date", "2024-05", "2024-05-01T25:00:00Z", "2024-05-01T12:61:00Z"] {
      assert_eq!(parse_rfc3339(text), None, "expected '{}' to be unreadable", text);
    }
  }

  #[test]
  fn days_from_civil_matches_known_dates() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(1969, 12, 31), -1);
    assert_eq!(days_from_civil(2000, 3, 1), 11017);
    assert_eq!(days_from_civil(2024, 5, 1), 19844);
  }

  fn unix(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
  }
}
