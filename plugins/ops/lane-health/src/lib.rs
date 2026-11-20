//! Deterministic port of the vault's implementation-lane health policy.
//!
//! The caller supplies task history, the current time, and the agent's
//! configuration update time. This crate never reads the Multica CLI or the
//! filesystem and never performs side effects.

use std::cmp::Ordering;

use agentmesh_ops_kit::OpsError;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde_json::{json, Value};

/// Recognized provider/infrastructure failure messages, in Python regex
/// alternative order. Matching is ASCII case-insensitive; numeric codes have
/// the same word-boundary requirement as the source expression.
#[derive(Debug, Clone, Copy)]
pub struct InfraPattern;

/// Compiled-pattern equivalent used by the source module's `INFRA` constant.
pub const INFRA: InfraPattern = InfraPattern;

/// Run statuses that count as completed history entries.
pub const TERMINAL: &[&str] = &["completed", "failed", "error", "cancelled", "canceled"];
/// Number of latest terminal runs needed to hold a lane.
pub const STREAK: usize = 3;
/// Maximum age of a run in a held-lane streak.
pub const WINDOW: chrono::Duration = chrono::Duration::hours(3);

const INFRA_ALTERNATIVES: &[&str] = &[
    "402",
    "500",
    "502",
    "503",
    "504",
    "429",
    "insufficient_balance",
    "insufficient balance",
    "membership",
    "billing",
    "quota",
    "rate_limit",
    "rate limit",
    "coding_plan",
    "coding plan",
    "free_usage_limit",
    "free usage limit",
    "FreeUsageLimit",
    "overloaded",
    "service unavailable",
    "ServiceUnavailable",
    "internal service failure",
    "payment required",
];

impl InfraPattern {
    /// Return the first matching substring, like `re.Pattern.search`.
    #[must_use]
    pub fn find<'a>(&self, text: &'a str) -> Option<&'a str> {
        let folded = text.to_ascii_lowercase();
        for (start, _) in text.char_indices() {
            for alternative in INFRA_ALTERNATIVES {
                let end = start.checked_add(alternative.len())?;
                let Some(candidate) = folded.get(start..end) else {
                    continue;
                };
                if !candidate.eq_ignore_ascii_case(alternative) {
                    continue;
                }
                if alternative
                    .chars()
                    .all(|character| character.is_ascii_digit())
                {
                    let before_is_word = text[..start]
                        .chars()
                        .next_back()
                        .is_some_and(is_word_character);
                    let after_is_word = text[end..].chars().next().is_some_and(is_word_character);
                    if before_is_word || after_is_word {
                        continue;
                    }
                }
                return text.get(start..end);
            }
        }
        None
    }

    /// Return whether the text contains a recognized infrastructure failure.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.find(text).is_some()
    }
}

fn is_word_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

/// A parsed ISO timestamp with Python's distinction between naive and
/// timezone-aware datetimes retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedDateTime {
    local: NaiveDateTime,
    offset_micros: Option<i64>,
}

impl ParsedDateTime {
    fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self.offset_micros, other.offset_micros) {
            (None, None) => Some(self.local.cmp(&other.local)),
            (Some(left_offset), Some(right_offset)) => {
                let left = self.local.and_utc().timestamp_micros() - left_offset;
                let right = other.local.and_utc().timestamp_micros() - right_offset;
                Some(left.cmp(&right))
            }
            _ => None,
        }
    }

    fn epoch_micros(self) -> Option<i64> {
        self.local
            .and_utc()
            .timestamp_micros()
            .checked_sub(self.offset_micros.unwrap_or(0))
    }

    /// Format the parsed value like Python `datetime.isoformat()`.
    #[must_use]
    pub fn isoformat(self) -> String {
        let mut result = format!(
            "{}T{}",
            self.local.date().format("%Y-%m-%d"),
            self.local.time().format("%H:%M:%S")
        );
        let micros = self.local.time().nanosecond() / 1_000;
        if micros != 0 {
            let mut fraction = format!("{micros:06}");
            while fraction.ends_with('0') {
                fraction.pop();
            }
            result.push('.');
            result.push_str(&fraction);
        }
        if let Some(offset_micros) = self.offset_micros {
            let sign = if offset_micros < 0 { '-' } else { '+' };
            let absolute = offset_micros.unsigned_abs();
            let total_seconds = absolute / 1_000_000;
            let hours = total_seconds / 3_600;
            let minutes = (total_seconds % 3_600) / 60;
            let seconds = total_seconds % 60;
            result.push_str(&format!("{sign}{hours:02}:{minutes:02}"));
            if seconds != 0 || absolute % 1_000_000 != 0 {
                result.push_str(&format!(":{seconds:02}"));
                let fractional = absolute % 1_000_000;
                if fractional != 0 {
                    let mut fraction = format!("{fractional:06}");
                    while fraction.ends_with('0') {
                        fraction.pop();
                    }
                    result.push('.');
                    result.push_str(&fraction);
                }
            }
        }
        result
    }
}

/// Parse an ISO date/time value using the source module's string conversion
/// and uppercase-`Z` normalization. Invalid values fail open as `None`.
#[must_use]
pub fn parse(value: &Value) -> Option<ParsedDateTime> {
    let text = python_string(value).replace('Z', "+00:00");
    parse_iso_datetime(&text)
}

fn parse_iso_datetime(text: &str) -> Option<ParsedDateTime> {
    let date_len = if text.as_bytes().get(4) == Some(&b'-') {
        10
    } else if text.as_bytes().get(4).is_some_and(u8::is_ascii_digit) {
        8
    } else {
        return None;
    };
    if text.len() < date_len {
        return None;
    }
    let date_text = text.get(..date_len)?;
    let date = if date_len == 10 {
        NaiveDate::parse_from_str(date_text, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(date_text, "%Y-%m"))
            .ok()?
    } else {
        NaiveDate::parse_from_str(date_text, "%Y%m%d")
            .or_else(|_| NaiveDate::parse_from_str(date_text, "%Y%m"))
            .ok()?
    };
    if text.len() == date_len {
        return Some(ParsedDateTime {
            local: date.and_time(NaiveTime::MIN),
            offset_micros: None,
        });
    }

    let separator = text.get(date_len..)?.chars().next()?;
    let time_start = date_len + separator.len_utf8();
    let time_and_offset = text.get(time_start..)?;
    let offset_start = time_and_offset
        .char_indices()
        .find(|(_, character)| *character == '+' || *character == '-')
        .map(|(index, _)| index);
    let (time_text, offset_micros) = match offset_start {
        Some(index) => (
            time_and_offset.get(..index)?,
            Some(parse_utc_offset(time_and_offset.get(index..)?)?),
        ),
        None => (time_and_offset, None),
    };
    let normalized_time = time_text.replace(',', ".");
    let time = [
        "%H:%M:%S%.f",
        "%H:%M:%S",
        "%H:%M",
        "%H%M%S%.f",
        "%H%M%S",
        "%H%M",
    ]
    .iter()
    .find_map(|format| NaiveTime::parse_from_str(&normalized_time, format).ok())?;
    let time = time.with_nanosecond((time.nanosecond() / 1_000) * 1_000)?;
    Some(ParsedDateTime {
        local: date.and_time(time),
        offset_micros,
    })
}

fn parse_utc_offset(text: &str) -> Option<i64> {
    let (sign, value) = match text.as_bytes().first()? {
        b'+' => (1_i64, text.get(1..)?),
        b'-' => (-1_i64, text.get(1..)?),
        _ => return None,
    };
    let (whole, fraction) = value
        .split_once('.')
        .or_else(|| value.split_once(','))
        .map_or((value, None), |(whole, fraction)| (whole, Some(fraction)));
    let digits: String = whole
        .chars()
        .filter(|character| *character != ':')
        .collect();
    let (hours, minutes, seconds) = match digits.len() {
        2 => (digits.get(..2)?.parse::<i64>().ok()?, 0_i64, 0_i64),
        4 => (
            digits.get(..2)?.parse::<i64>().ok()?,
            digits.get(2..4)?.parse::<i64>().ok()?,
            0_i64,
        ),
        6 => (
            digits.get(..2)?.parse::<i64>().ok()?,
            digits.get(2..4)?.parse::<i64>().ok()?,
            digits.get(4..6)?.parse::<i64>().ok()?,
        ),
        _ => return None,
    };
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let mut fractional_micros = 0_i64;
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.chars().all(|character| character.is_ascii_digit()) {
            return None;
        }
        let micros = fraction.chars().take(6).collect::<String>();
        let parsed = micros.parse::<i64>().ok()?;
        fractional_micros = parsed * 10_i64.pow(u32::try_from(6 - micros.len()).ok()?);
    }
    let offset = (((hours * 60 + minutes) * 60 + seconds) * 1_000_000) + fractional_micros;
    (offset < 24 * 60 * 60 * 1_000_000).then_some(sign * offset)
}

fn python_string(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// Return the source module's provider-failure hold reason, if the latest
/// terminal streak is entirely infrastructure failures within `WINDOW`.
#[must_use]
pub fn infra_failure(tasks: &[Value], now: &ParsedDateTime) -> Option<String> {
    let mut recent = Vec::new();
    for task in tasks {
        let status = task
            .get("status")
            .filter(|value| is_truthy(value))
            .map(python_string)
            .unwrap_or_default()
            .to_lowercase();
        if !TERMINAL.contains(&status.as_str()) {
            continue;
        }
        let timestamp = task
            .get("completed_at")
            .filter(|value| is_truthy(value))
            .or_else(|| task.get("created_at"))
            .and_then(parse);
        let Some(timestamp) = timestamp else {
            continue;
        };
        now.compare(&timestamp)?;
        let delta = now.epoch_micros()?.checked_sub(timestamp.epoch_micros()?)?;
        if delta <= WINDOW.num_microseconds()? {
            recent.push((timestamp, task));
        }
    }
    recent.sort_by(|left, right| right.0.compare(&left.0).unwrap_or(Ordering::Equal));
    let latest = recent.into_iter().take(STREAK).collect::<Vec<_>>();
    if latest.len() < STREAK {
        return None;
    }
    let statuses_are_failures = latest.iter().all(|(_, task)| {
        ["failed", "error"].contains(
            &task
                .get("status")
                .map(python_string)
                .unwrap_or_else(|| "None".to_owned())
                .to_lowercase()
                .as_str(),
        )
    });
    if !statuses_are_failures {
        return None;
    }
    let errors = latest
        .iter()
        .map(|(_, task)| {
            task.get("error")
                .filter(|value| is_truthy(value))
                .map(python_string)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    if !errors.iter().all(|error| INFRA.is_match(error)) {
        return None;
    }
    INFRA
        .find(&errors[0])
        .map(|match_text| format!("lane_infra_failing:{}", match_text.to_lowercase()))
}

/// Drop tasks created on or before the agent configuration update time.
/// An unparsable update timestamp leaves the input unchanged, as in Python.
#[must_use]
pub fn since_config(tasks: &[Value], updated_at: &Value) -> Vec<Value> {
    let Some(cut) = parse(updated_at) else {
        return tasks.to_vec();
    };
    let mut filtered = Vec::new();
    for task in tasks {
        let created = task.get("created_at").and_then(parse).unwrap_or(cut);
        match created.compare(&cut) {
            Some(Ordering::Greater) => filtered.push(task.clone()),
            Some(_) => {}
            // `checker` preserves the unfiltered history if a mixed
            // naive/aware timestamp raises during Python's filtering step.
            None => return tasks.to_vec(),
        }
    }
    filtered
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn task_array(request: &Value) -> Result<&[Value], OpsError> {
    request
        .get("tasks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| OpsError::invalid_request("tasks must be an array"))
}

fn required_parse(request: &Value, key: &str) -> Result<ParsedDateTime, OpsError> {
    let value = request
        .get(key)
        .ok_or_else(|| OpsError::invalid_request(format!("{key} is required")))?;
    parse(value).ok_or_else(|| OpsError::invalid_request(format!("{key} must be an ISO timestamp")))
}

/// Handle stdio operations over the Python helper inputs.
///
/// `operation` is `parse`, `infra_failure`, `since_config`, or `checker`.
/// Task history is always supplied as JSON data by the caller.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    if !request.is_object() {
        return Err(OpsError::invalid_request("request must be an object"));
    }
    let operation = request
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("operation must be a string"))?;
    match operation {
        "parse" => {
            let value = request
                .get("value")
                .ok_or_else(|| OpsError::invalid_request("value is required"))?;
            Ok(json!({
                "parsed": parse(value).map(ParsedDateTime::isoformat),
            }))
        }
        "infra_failure" => {
            let tasks = task_array(&request)?;
            let now = required_parse(&request, "now")?;
            Ok(json!({ "reason": infra_failure(tasks, &now) }))
        }
        "since_config" => {
            let tasks = task_array(&request)?;
            let updated_at = request.get("updated_at").cloned().unwrap_or(Value::Null);
            Ok(json!({ "tasks": since_config(tasks, &updated_at) }))
        }
        "checker" => {
            let tasks = task_array(&request)?;
            let now = required_parse(&request, "now")?;
            let updated_at = request.get("updated_at").cloned().unwrap_or(Value::Null);
            let filtered = since_config(tasks, &updated_at);
            Ok(json!({ "hold": infra_failure(&filtered, &now) }))
        }
        _ => Err(OpsError::invalid_request(
            "operation must be parse, infra_failure, since_config, or checker",
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse, INFRA, STREAK, TERMINAL, WINDOW};

    #[test]
    fn infra_pattern_obeys_regex_boundaries_and_case_insensitivity() {
        assert_eq!(
            INFRA.find("Internal Service Failure"),
            Some("Internal Service Failure")
        );
        assert_eq!(INFRA.find("code=429"), Some("429"));
        assert_eq!(INFRA.find("1429x"), None);
        assert_eq!(
            INFRA.find("service unavailable"),
            Some("service unavailable")
        );
    }

    #[test]
    fn constants_match_the_python_policy() {
        assert_eq!(STREAK, 3);
        assert_eq!(
            TERMINAL,
            &["completed", "failed", "error", "cancelled", "canceled"]
        );
        assert_eq!(WINDOW.num_hours(), 3);
    }

    #[test]
    fn parsing_keeps_naive_and_offset_timestamps_distinct() {
        let naive = parse(&json!("2026-11-12T10:00:00.123456789")).expect("valid naive timestamp");
        assert_eq!(naive.isoformat(), "2026-11-12T10:00:00.123456");
        let aware = parse(&json!("2026-11-12T10:00:00Z")).expect("valid aware timestamp");
        assert_eq!(aware.isoformat(), "2026-11-12T10:00:00+00:00");
        assert!(parse(&json!("not-a-timestamp")).is_none());
    }
}
