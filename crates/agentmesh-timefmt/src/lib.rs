//! RFC 3339 helpers shared by the plugins.
//!
//! The workspace used to format timestamps with chrono's `to_rfc3339()`.
//! The `time` crate's own RFC 3339 formatter prints `Z` for UTC and trims
//! trailing fractional zeros, so these helpers reproduce the previous text
//! exactly: an offset is always `+HH:MM` / `-HH:MM`, and fractional seconds
//! appear only when non-zero, as 3, 6 or 9 digits.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Parse an RFC 3339 timestamp, keeping its original UTC offset.
///
/// # Errors
///
/// Returns the `time` parse error when `value` is not an RFC 3339 timestamp.
pub fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, time::error::Parse> {
    OffsetDateTime::parse(value, &Rfc3339)
}

/// Format like chrono's `DateTime::to_rfc3339()`.
#[must_use]
pub fn to_rfc3339(value: OffsetDateTime) -> String {
    let mut text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
    );
    let nanos = value.nanosecond();
    if nanos != 0 {
        if nanos.is_multiple_of(1_000_000) {
            text.push_str(&format!(".{:03}", nanos / 1_000_000));
        } else if nanos.is_multiple_of(1_000) {
            text.push_str(&format!(".{:06}", nanos / 1_000));
        } else {
            text.push_str(&format!(".{nanos:09}"));
        }
    }
    let offset_seconds = value.offset().whole_seconds();
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute = offset_seconds.unsigned_abs();
    text.push_str(&format!(
        "{sign}{:02}:{:02}",
        absolute / 3_600,
        (absolute % 3_600) / 60
    ));
    text
}

/// The calendar date as `YYYY-MM-DD`, in the value's own offset.
#[must_use]
pub fn date_string(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    #[test]
    fn formats_whole_seconds_with_numeric_offset() {
        let parsed = parse_rfc3339("2026-08-30T12:00:00+09:00").unwrap();
        assert_eq!(to_rfc3339(parsed), "2026-08-30T12:00:00+09:00");
    }

    #[test]
    fn utc_uses_plus_zero_offset_not_z() {
        let parsed = parse_rfc3339("2026-08-30T12:00:00Z").unwrap();
        assert_eq!(to_rfc3339(parsed), "2026-08-30T12:00:00+00:00");
    }

    #[test]
    fn negative_offset_is_preserved() {
        let parsed = parse_rfc3339("2026-01-02T03:04:05-07:30").unwrap();
        assert_eq!(to_rfc3339(parsed), "2026-01-02T03:04:05-07:30");
    }

    #[test]
    fn fraction_uses_three_six_or_nine_digits() {
        let ms = parse_rfc3339("2026-08-30T12:00:00.5+09:00").unwrap();
        assert_eq!(to_rfc3339(ms), "2026-08-30T12:00:00.500+09:00");
        let us = parse_rfc3339("2026-08-30T12:00:00.123456Z").unwrap();
        assert_eq!(to_rfc3339(us), "2026-08-30T12:00:00.123456+00:00");
        let ns = parse_rfc3339("2026-08-30T12:00:00.000000001Z").unwrap();
        assert_eq!(to_rfc3339(ns), "2026-08-30T12:00:00.000000001+00:00");
    }

    #[test]
    fn arithmetic_keeps_offset() {
        let parsed = parse_rfc3339("2026-08-30T12:00:00+09:00").unwrap();
        assert_eq!(
            to_rfc3339(parsed + Duration::days(7)),
            "2026-09-06T12:00:00+09:00"
        );
        assert_eq!(
            to_rfc3339(parsed - Duration::seconds(3_600)),
            "2026-08-30T11:00:00+09:00"
        );
    }

    #[test]
    fn date_string_uses_the_value_offset() {
        let parsed = parse_rfc3339("2026-08-30T23:30:00-05:00").unwrap();
        assert_eq!(date_string(parsed), "2026-08-30");
    }

    #[test]
    fn rejects_non_rfc3339_text() {
        assert!(parse_rfc3339("2026-08-30").is_err());
        assert!(parse_rfc3339("not a time").is_err());
        assert!(parse_rfc3339("2026-08-30T12:00:00").is_err());
    }
}
