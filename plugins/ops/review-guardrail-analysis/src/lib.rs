//! Review guardrail promotion analysis ported from
//! `report-review-guardrail-promotions.py`.
//!
//! The crate ports `parse_time`, `occurrence_time` and `analyze`. Callers pass
//! synthetic evidence records, a registry object and `now` as JSON values.
//! `load_evidence`, report and issue file generation,
//! `create_candidate_issues` and `main` stay in the vault wrapper: the crate
//! has no file, shell, network, Multica or git side effects.
//!
//! Python raises `ValueError`, `TypeError`, `AttributeError` or `KeyError` on
//! malformed input. Those cases are reported as an `invalid_request` error with
//! [`MALFORMED_MESSAGE`].
//!
//! Known bounds: timestamps accept `YYYY-MM-DD`, an optional `T` or space and
//! `HH[:MM[:SS[.f+]]]`, then an optional `Z`, `+HH:MM[:SS]` or `+HHMM` offset
//! (fractions are truncated to microseconds; week dates, basic dates and
//! comma fractions are rejected). Timestamps that are not strings are treated
//! as unparseable. `int()` text is limited to an optional sign and ASCII
//! digits, and `str()` of a non-string scalar is limited to integers and
//! booleans.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// Message used where Python raises on a malformed value or container shape.
pub const MALFORMED_MESSAGE: &str = "malformed input value";

static NULL: Value = Value::Null;

const MICROS_PER_SECOND: i128 = 1_000_000;
const MICROS_PER_DAY: i128 = 86_400 * MICROS_PER_SECOND;
const MAX_DAYS: i64 = 1_000_000_000;

fn malformed() -> OpsError {
    OpsError::invalid_request(MALFORMED_MESSAGE)
}

/// Python truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_none_or(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `first or second` on JSON values.
fn or<'a>(first: &'a Value, second: &'a Value) -> &'a Value {
    if truthy(first) {
        first
    } else {
        second
    }
}

/// `str(value)` for strings, integers and booleans; other values are malformed.
fn py_str(value: &Value) -> Result<String, OpsError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Bool(true) => Ok("True".to_owned()),
        Value::Bool(false) => Ok("False".to_owned()),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(number.to_string()),
        _ => Err(malformed()),
    }
}

/// `int(value or default)`.
fn int_or(value: &Value, default: i64) -> Result<i64, OpsError> {
    if !truthy(value) {
        return Ok(default);
    }
    match value {
        Value::Bool(_) => Ok(1),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(int)
            } else if number.is_u64() {
                Err(malformed())
            } else {
                // `int(float)` truncates toward zero.
                let float = number.as_f64().ok_or_else(malformed)?;
                if float.abs() < 9.0e18 {
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(float.trunc() as i64)
                } else {
                    Err(malformed())
                }
            }
        }
        Value::String(text) => {
            let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(malformed());
            }
            text.parse::<i64>().map_err(|_| malformed())
        }
        _ => Err(malformed()),
    }
}

/// A timezone-aware instant: UTC microseconds since the Unix epoch plus the
/// UTC offset it was written with (kept so `isoformat` output matches Python).
#[derive(Debug, Clone, Copy)]
struct Moment {
    micros: i128,
    offset_seconds: i32,
}

impl PartialEq for Moment {
    fn eq(&self, other: &Self) -> bool {
        self.micros == other.micros
    }
}

impl Eq for Moment {}

impl PartialOrd for Moment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Moment {
    fn cmp(&self, other: &Self) -> Ordering {
        self.micros.cmp(&other.micros)
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Proleptic Gregorian date for days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Exactly `len` ASCII digits at the start of `text`.
fn take_digits(text: &str, len: usize) -> Option<(i64, &str)> {
    let head = text.get(..len)?;
    if !head.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((head.parse().ok()?, &text[len..]))
}

/// `[+-]HH[:MM[:SS]]` or `[+-]HHMM` after the time; `None` when malformed.
fn parse_offset(text: &str) -> Option<i32> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    let (hours, rest) = take_digits(rest, 2)?;
    let (minutes, seconds) = if rest.is_empty() {
        (0, 0)
    } else if let Some(rest) = rest.strip_prefix(':') {
        let (minutes, rest) = take_digits(rest, 2)?;
        if rest.is_empty() {
            (minutes, 0)
        } else {
            let (seconds, rest) = take_digits(rest.strip_prefix(':')?, 2)?;
            if !rest.is_empty() {
                return None;
            }
            (minutes, seconds)
        }
    } else {
        let (minutes, rest) = take_digits(rest, 2)?;
        if !rest.is_empty() {
            return None;
        }
        (minutes, 0)
    };
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    let total = hours * 3600 + minutes * 60 + seconds;
    i32::try_from(total).ok().map(|total| total * sign)
}

/// `parse_time`: ISO text with `Z` as UTC; a naive value is read as UTC.
fn parse_time(value: &str) -> Result<Moment, OpsError> {
    let text = value.replace('Z', "+00:00");
    let (year, rest) = take_digits(&text, 4).ok_or_else(malformed)?;
    let (month, rest) =
        take_digits(rest.strip_prefix('-').ok_or_else(malformed)?, 2).ok_or_else(malformed)?;
    let (day, rest) =
        take_digits(rest.strip_prefix('-').ok_or_else(malformed)?, 2).ok_or_else(malformed)?;
    if year < 1 || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return Err(malformed());
    }
    let mut seconds_of_day = 0_i64;
    let mut fraction = 0_i64;
    let mut offset_seconds = 0_i32;
    if !rest.is_empty() {
        let mut chars = rest.chars();
        if !matches!(chars.next(), Some('T' | ' ')) {
            return Err(malformed());
        }
        let rest = chars.as_str();
        let (hour, mut rest) = take_digits(rest, 2).ok_or_else(malformed)?;
        let (mut minute, mut second) = (0, 0);
        if let Some(after) = rest.strip_prefix(':') {
            let (value, after) = take_digits(after, 2).ok_or_else(malformed)?;
            minute = value;
            rest = after;
            if let Some(after) = rest.strip_prefix(':') {
                let (value, after) = take_digits(after, 2).ok_or_else(malformed)?;
                second = value;
                rest = after;
                if let Some(after) = rest.strip_prefix('.') {
                    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
                    if digits == 0 {
                        return Err(malformed());
                    }
                    let mut padded: String = after[..digits].chars().take(6).collect();
                    while padded.len() < 6 {
                        padded.push('0');
                    }
                    fraction = padded.parse().map_err(|_| malformed())?;
                    rest = &after[digits..];
                }
            }
        }
        // Python 3.14 reads exactly `24:00:00` (no fraction) as the next midnight.
        let midnight_24 = hour == 24 && minute == 0 && second == 0 && fraction == 0;
        if (hour > 23 && !midnight_24) || minute > 59 || second > 59 {
            return Err(malformed());
        }
        seconds_of_day = hour * 3600 + minute * 60 + second;
        if !rest.is_empty() {
            offset_seconds = parse_offset(rest).ok_or_else(malformed)?;
        }
    }
    let days = days_from_civil(year, month, day);
    let local = i128::from(days) * MICROS_PER_DAY
        + i128::from(seconds_of_day) * MICROS_PER_SECOND
        + i128::from(fraction);
    Ok(Moment {
        micros: local - i128::from(offset_seconds) * MICROS_PER_SECOND,
        offset_seconds,
    })
}

/// Python `datetime.isoformat()` of an aware datetime.
fn isoformat(moment: Moment) -> Result<String, OpsError> {
    let local = moment.micros + i128::from(moment.offset_seconds) * MICROS_PER_SECOND;
    let days = i64::try_from(local.div_euclid(MICROS_PER_DAY)).map_err(|_| malformed())?;
    let in_day = local.rem_euclid(MICROS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    if !(1..=9999).contains(&year) {
        return Err(malformed());
    }
    let seconds = in_day / MICROS_PER_SECOND;
    let micros = in_day % MICROS_PER_SECOND;
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if micros != 0 {
        text.push_str(&format!(".{micros:06}"));
    }
    let offset = moment.offset_seconds;
    let magnitude = offset.abs();
    text.push(if offset < 0 { '-' } else { '+' });
    text.push_str(&format!(
        "{:02}:{:02}",
        magnitude / 3600,
        magnitude / 60 % 60
    ));
    if magnitude % 60 != 0 {
        text.push_str(&format!(":{:02}", magnitude % 60));
    }
    Ok(text)
}

fn string_arg<'a>(request: &'a Map<String, Value>, field: &str) -> Result<&'a str, OpsError> {
    request
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(malformed)
}

/// `occurrence_time`: the finding creation time, falling back to the
/// observation time; `None` when the chosen value does not parse.
fn occurrence_time(item: &Map<String, Value>) -> Option<Moment> {
    let created = item.get("finding_created_at").unwrap_or(&NULL);
    let observed = item.get("observed_at").unwrap_or(&NULL);
    match or(created, observed) {
        Value::String(text) => parse_time(text).ok(),
        _ => None,
    }
}

fn as_object(value: &Value) -> Result<&Map<String, Value>, OpsError> {
    value.as_object().ok_or_else(malformed)
}

/// `value or []` as a list of items.
fn list_or_empty(value: &Value) -> Result<&[Value], OpsError> {
    if !truthy(value) {
        return Ok(&[]);
    }
    value.as_array().map(Vec::as_slice).ok_or_else(malformed)
}

/// `analyze`: rolling-window grouping, thresholds and promotion candidates.
fn analyze(
    records: &[Value],
    registry: &Map<String, Value>,
    now: Moment,
) -> Result<Value, OpsError> {
    let get = |field: &str| registry.get(field).unwrap_or(&NULL);
    let window_days = int_or(get("promotion_window_days"), 90)?;
    if window_days.abs() > MAX_DAYS {
        return Err(malformed());
    }
    let cutoff = Moment {
        micros: now.micros - i128::from(window_days) * MICROS_PER_DAY,
        offset_seconds: now.offset_seconds,
    };
    let minimum_findings = int_or(get("minimum_findings"), 3)?;
    let minimum_repositories = int_or(get("minimum_repositories"), 2)?;

    let mut rules: BTreeMap<String, &Map<String, Value>> = BTreeMap::new();
    for entry in list_or_empty(get("guardrails"))? {
        let entry = as_object(entry)?;
        let key = entry
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        rules.insert(key.to_owned(), entry);
    }

    let mut grouped: BTreeMap<String, Vec<&Map<String, Value>>> = BTreeMap::new();
    for record in records {
        let item = as_object(record)?;
        let key = py_str(or(
            item.get("normalized_guardrail_key").unwrap_or(&NULL),
            &json!("unclassified"),
        ))?;
        let Some(when) = occurrence_time(item) else {
            continue;
        };
        if when >= cutoff {
            grouped.entry(key).or_default().push(item);
        }
    }

    let keys: BTreeSet<&String> = rules.keys().chain(grouped.keys()).collect();
    let empty_rule = Map::new();
    let mut rows = Vec::new();
    let mut candidates = Vec::new();
    for key in keys {
        let items: &[&Map<String, Value>] = grouped.get(key).map_or(&[], Vec::as_slice);
        let mut repos = BTreeSet::new();
        for item in items {
            let repo = item.get("repo").unwrap_or(&NULL);
            if truthy(repo) {
                repos.insert(py_str(repo)?);
            }
        }
        let immediate = items
            .iter()
            .any(|item| item.get("immediate_candidate") == Some(&Value::Bool(true)));
        let rule = rules.get(key).copied().unwrap_or(&empty_rule);
        let finding_count = i64::try_from(items.len()).map_err(|_| malformed())?;
        let repository_count = i64::try_from(repos.len()).map_err(|_| malformed())?;
        let threshold_met =
            finding_count >= minimum_findings && repository_count >= minimum_repositories;
        let prevention_status = py_str(or(
            rule.get("prevention_status").unwrap_or(&NULL),
            &json!("unregistered"),
        ))?;
        let recurrence_since = or(
            rule.get("prevention_promoted_at").unwrap_or(&NULL),
            rule.get("cataloged_at").unwrap_or(&NULL),
        );
        let mut recurrence_count = 0_i64;
        if truthy(recurrence_since) {
            let since = parse_time(&py_str(recurrence_since)?)?;
            let count = items
                .iter()
                .filter(|item| occurrence_time(item).is_some_and(|when| when >= since))
                .count();
            recurrence_count = i64::try_from(count).map_err(|_| malformed())?;
        }
        let title = or(rule.get("title").unwrap_or(&NULL), &json!(key)).clone();
        let row = json!({
            "key": key,
            "title": title,
            "finding_count": finding_count,
            "repository_count": repository_count,
            "repositories": repos,
            "threshold_met": threshold_met,
            "immediate_candidate": immediate,
            "prevention_status": prevention_status,
            "prevention_target": rule.get("prevention_target").unwrap_or(&NULL),
            "recurrence_count": recurrence_count,
        });
        if key != "unclassified"
            && matches!(prevention_status.as_str(), "catalog_only" | "unregistered")
            && (threshold_met || immediate)
        {
            candidates.push(row.clone());
        }
        rows.push(row);
    }
    Ok(json!({
        "window_days": window_days,
        "cutoff": isoformat(cutoff)?,
        "rows": rows,
        "promotion_candidates": candidates,
    }))
}

/// Handle one request: an object with an `op` string.
///
/// Operations: `parse_time` (`value`), `occurrence_time` (`item`) and
/// `analyze` (`records`, `registry`, `now`).
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be a JSON object"))?;
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("op must be a string"))?;
    match op {
        "parse_time" => Ok(json!(isoformat(parse_time(string_arg(
            request, "value"
        )?)?)?)),
        "occurrence_time" => {
            let item = as_object(request.get("item").unwrap_or(&NULL))?;
            occurrence_time(item).map_or(Ok(Value::Null), |moment| Ok(json!(isoformat(moment)?)))
        }
        "analyze" => {
            let records = request
                .get("records")
                .and_then(Value::as_array)
                .ok_or_else(malformed)?;
            let registry = as_object(request.get("registry").unwrap_or(&NULL))?;
            let now = parse_time(string_arg(request, "now")?)?;
            analyze(records, registry, now)
        }
        other => Err(OpsError::invalid_request(format!("unknown op: {other}"))),
    }
}
