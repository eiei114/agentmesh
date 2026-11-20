//! Deterministic port of the Autopilot heartbeat event and watchdog helpers.
//!
//! Wall-clock access, session-file access, and controller side effects are
//! intentionally absent. Callers supply timestamps and progress summaries.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Map, Value};

use agentmesh_ops_kit::OpsError;

/// Prefix used by heartbeat JSON lines.
pub const HEARTBEAT_PREFIX: &str = "MULTICA_HEARTBEAT ";
/// Accepted event types, including terminal results.
pub const HEARTBEAT_EVENT_TYPES: &[&str] = &[
    "phase_start",
    "tool_in_flight",
    "provider_wait",
    "progress",
    "terminal_result",
];
/// Events that count as watchdog activity. A terminal result is not activity.
pub const ACTIVITY_HEARTBEAT_EVENTS: &[&str] =
    &["phase_start", "tool_in_flight", "provider_wait", "progress"];
/// Default watchdog idle threshold in seconds.
pub const DEFAULT_WATCHDOG_IDLE_THRESHOLD_SECONDS: i64 = 15 * 60;
/// Default interval for bounded progress reporting, in seconds.
pub const DEFAULT_BOUNDED_PROGRESS_INTERVAL_SECONDS: i64 = 5 * 60;

/// A validated heartbeat event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatEvent {
    /// Event kind from [`HEARTBEAT_EVENT_TYPES`].
    pub event: String,
    /// Controller phase.
    pub phase: String,
    /// Controller name.
    pub controller: String,
    /// Timestamp supplied by the caller.
    pub ts: String,
    /// Optional event detail.
    pub detail: Option<String>,
    /// Optional tool name.
    pub tool: Option<String>,
    /// Optional session path, carried as data only and never opened.
    pub session_path: Option<String>,
}

impl HeartbeatEvent {
    /// Construct an event using the Python implementation's validation rules.
    pub fn from_dict(payload: &Value) -> Option<Self> {
        let object = payload.as_object()?;
        let event = python_string_or_empty(object.get("event"));
        let phase = python_string_or_empty(object.get("phase"));
        let controller = python_string_or_empty(object.get("controller"));
        let ts = python_string_or_empty(object.get("ts"));
        if !HEARTBEAT_EVENT_TYPES.contains(&event.as_str())
            || phase.is_empty()
            || controller.is_empty()
            || ts.is_empty()
        {
            return None;
        }
        Some(Self {
            event,
            phase,
            controller,
            ts,
            detail: python_optional_string(object.get("detail")),
            tool: python_optional_string(object.get("tool")),
            session_path: python_optional_string(object.get("session_path")),
        })
    }

    /// Parse one prefixed JSON heartbeat line.
    pub fn from_line(line: &str) -> Option<Self> {
        let text = line.trim();
        let payload = if let Some(payload) = text.strip_prefix(HEARTBEAT_PREFIX) {
            payload.trim()
        } else {
            let (prefix, payload) = text.split_once(char::is_whitespace)?;
            if prefix != "MULTICA_HEARTBEAT" || payload.trim().is_empty() {
                return None;
            }
            payload.trim()
        };
        let value: Value = serde_json::from_str(payload).ok()?;
        Self::from_dict(&value)
    }

    /// Convert to the event JSON object, omitting absent optional fields.
    #[must_use]
    pub fn to_dict(&self) -> Value {
        let mut object = Map::new();
        object.insert("event".into(), Value::String(self.event.clone()));
        object.insert("phase".into(), Value::String(self.phase.clone()));
        object.insert("controller".into(), Value::String(self.controller.clone()));
        object.insert("ts".into(), Value::String(self.ts.clone()));
        insert_optional(&mut object, "detail", &self.detail);
        insert_optional(&mut object, "tool", &self.tool);
        insert_optional(&mut object, "session_path", &self.session_path);
        Value::Object(object)
    }

    /// Serialize with the Python line contract's sorted keys and spacing.
    #[must_use]
    pub fn to_line(&self) -> String {
        let object = self.to_dict();
        let sorted = [
            "controller",
            "detail",
            "event",
            "phase",
            "session_path",
            "tool",
            "ts",
        ];
        let fields = sorted
            .iter()
            .filter_map(|key| {
                object.get(*key).map(|value| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(key).expect("string keys serialize"),
                        serde_json::to_string(value).expect("JSON values serialize")
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{HEARTBEAT_PREFIX}{{{fields}}}")
    }
}

fn insert_optional(object: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), Value::String(value.clone()));
    }
}

fn python_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Number(value)) => value.as_f64().is_some_and(|number| number != 0.0),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

fn python_string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Array(value)) => format!("{value:?}"),
        Some(Value::Object(value)) => format!("{value:?}"),
    }
}

fn python_string_or_empty(value: Option<&Value>) -> String {
    if python_truthy(value) {
        python_string(value)
    } else {
        String::new()
    }
}

fn python_optional_string(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.is_empty() => None,
        Some(value) => Some(python_string(Some(value))),
    }
}

/// Parse an ISO timestamp and normalize it to UTC.
///
/// Naive timestamps are interpreted as UTC so results never depend on the
/// machine's local timezone. Callers should prefer timestamps with an offset.
pub fn parse_dt(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value.filter(|value| !value.is_empty())?;
    let mut normalized = value.to_owned();
    if normalized
        .as_bytes()
        .get(10)
        .is_some_and(|byte| *byte == b' ' || *byte == b't')
    {
        normalized.replace_range(10..11, "T");
    }
    if normalized.ends_with('Z') {
        normalized.truncate(normalized.len() - 1);
        normalized.push_str("+00:00");
    }
    normalized = normalize_compact_offset(&normalized);

    let parsed = [
        DateTime::parse_from_rfc3339(&normalized).ok(),
        DateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f%:z").ok(),
        DateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%:z").ok(),
    ]
    .into_iter()
    .flatten()
    .next()
    .map(|date| date.with_timezone(&Utc))
    .or_else(|| parse_offset_with_seconds(&normalized))
    .or_else(|| {
        let naive = NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S"))
            .ok()?;
        Some(DateTime::from_naive_utc_and_offset(naive, Utc))
    })?;
    let micros = parsed.timestamp_micros();
    DateTime::from_timestamp_micros(micros)
}

/// Parse ISO UTC offsets with seconds, which Python's `fromisoformat` accepts
/// but Chrono's RFC 3339 parser does not. Fractional offset seconds are kept at
/// Python datetime's microsecond precision.
fn parse_offset_with_seconds(value: &str) -> Option<DateTime<Utc>> {
    let offset_index = value.char_indices().skip(10).find_map(|(index, ch)| {
        if ch == '+' || ch == '-' {
            Some(index)
        } else {
            None
        }
    })?;
    let local_text = &value[..offset_index];
    let offset_text = &value[offset_index..];
    let (sign, offset_text) = match offset_text.as_bytes().first()? {
        b'+' => (1_i64, &offset_text[1..]),
        b'-' => (-1_i64, &offset_text[1..]),
        _ => return None,
    };
    let (offset_clock, fraction) = offset_text
        .split_once('.')
        .map_or((offset_text, None), |(clock, fraction)| {
            (clock, Some(fraction))
        });

    let (hours, minutes, seconds) = if offset_clock.contains(':') {
        let mut parts = offset_clock.split(':');
        let hours = parse_two_digits(parts.next()?)?;
        let minutes = parse_two_digits(parts.next()?)?;
        let seconds = parse_two_digits(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        (hours, minutes, seconds)
    } else {
        if offset_clock.len() != 6 || !offset_clock.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        (
            parse_two_digits(&offset_clock[..2])?,
            parse_two_digits(&offset_clock[2..4])?,
            parse_two_digits(&offset_clock[4..6])?,
        )
    };
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        return None;
    }

    let fraction_micros = match fraction {
        None => 0,
        Some(digits)
            if !digits.is_empty()
                && digits.len() <= 6
                && digits.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            let mut padded = digits.to_owned();
            padded.push_str(&"0".repeat(6 - digits.len()));
            padded.parse::<i64>().ok()?
        }
        Some(_) => return None,
    };

    let naive = NaiveDateTime::parse_from_str(local_text, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    let offset_micros = sign
        * (((i64::from(hours) * 60 + i64::from(minutes)) * 60 + i64::from(seconds)) * 1_000_000
            + fraction_micros);
    let utc_micros = naive
        .and_utc()
        .timestamp_micros()
        .checked_sub(offset_micros)?;
    DateTime::from_timestamp_micros(utc_micros)
}

fn parse_two_digits(value: &str) -> Option<u32> {
    if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

fn normalize_compact_offset(value: &str) -> String {
    let Some(index) = value.char_indices().skip(10).find_map(|(index, ch)| {
        if ch == '+' || ch == '-' {
            Some(index)
        } else {
            None
        }
    }) else {
        return value.to_owned();
    };
    let offset = &value[index..];
    match offset.len() {
        3 => format!("{}{}:00", &value[..index], offset),
        5 if !offset.contains(':') => {
            format!("{}{}:{}", &value[..index], &offset[..3], &offset[3..])
        }
        _ => value.to_owned(),
    }
}

fn format_dt(value: DateTime<Utc>) -> String {
    let base = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

/// Parse prefixed heartbeat lines, ignoring non-heartbeat and invalid lines.
pub fn parse_heartbeat_lines(text: Option<&str>) -> Vec<HeartbeatEvent> {
    text.unwrap_or_default()
        .lines()
        .filter_map(HeartbeatEvent::from_line)
        .collect()
}

/// Return the latest event by parsed timestamp, keeping the first on ties.
pub fn latest_heartbeat(events: &[HeartbeatEvent]) -> Option<&HeartbeatEvent> {
    let mut latest: Option<&HeartbeatEvent> = None;
    let mut latest_at: Option<DateTime<Utc>> = None;
    for event in events {
        let at = parse_dt(Some(&event.ts));
        if latest.is_none() || at > latest_at {
            latest = Some(event);
            latest_at = at;
        }
    }
    latest
}

/// Return the latest timestamp among heartbeat events that count as activity.
pub fn heartbeat_activity_at(events: &[HeartbeatEvent]) -> Option<DateTime<Utc>> {
    events
        .iter()
        .filter(|event| ACTIVITY_HEARTBEAT_EVENTS.contains(&event.event.as_str()))
        .filter_map(|event| parse_dt(Some(&event.ts)))
        .max()
}

fn last_run_message(messages: &[Value]) -> Option<&Value> {
    let mut latest: Option<&Value> = None;
    let mut latest_key = String::new();
    for message in messages {
        let value = message.get("created_at");
        let key = if python_truthy(value) {
            python_string(value)
        } else {
            String::new()
        };
        if latest.is_none() || key > latest_key {
            latest = Some(message);
            latest_key = key;
        }
    }
    latest
}

/// Return the timestamp of the lexicographically latest run-message summary.
pub fn run_message_activity_at(messages: &[Value]) -> Option<DateTime<Utc>> {
    let message = last_run_message(messages)?;
    let created_at = message.get("created_at");
    let value = if python_truthy(created_at) {
        python_string(created_at)
    } else {
        String::new()
    };
    parse_dt(Some(&value))
}

fn activity_delta_seconds(now: DateTime<Utc>, activity: DateTime<Utc>) -> f64 {
    (now.timestamp_micros() - activity.timestamp_micros()) as f64 / 1_000_000.0
}

/// Analyze watchdog activity using the caller's explicit clock and threshold.
pub fn analyze_watchdog_activity(
    heartbeats: &[HeartbeatEvent],
    run_messages: &[Value],
    session_progress: Option<&Value>,
    now: DateTime<Utc>,
    threshold_seconds: i64,
) -> Value {
    let last_hb = latest_heartbeat(heartbeats);
    let last_msg = last_run_message(run_messages);
    let hb_at = heartbeat_activity_at(heartbeats);
    let msg_at = run_message_activity_at(run_messages);
    let last_activity_at = match (hb_at, msg_at) {
        (Some(hb), Some(msg)) => Some(hb.max(msg)),
        (Some(hb), None) => Some(hb),
        (None, Some(msg)) => Some(msg),
        (None, None) => None,
    };
    let elapsed_seconds =
        last_activity_at.map(|activity| activity_delta_seconds(now, activity).max(0.0));

    let mut tool_in_flight =
        last_msg.is_some_and(|message| python_string_or_empty(message.get("type")) == "tool_use");
    let tool_use_age_seconds = if tool_in_flight {
        msg_at.map(|at| activity_delta_seconds(now, at).max(0.0))
    } else {
        None
    };
    if tool_use_age_seconds.is_some_and(|age| age >= threshold_seconds as f64) {
        tool_in_flight = false;
    }

    let last_three = &heartbeats[heartbeats.len().saturating_sub(3)..];
    let heartbeat_activity_recent =
        hb_at.is_some_and(|at| activity_delta_seconds(now, at) < threshold_seconds as f64);
    let recent_tool_in_flight_heartbeat = last_three
        .iter()
        .any(|event| event.event == "tool_in_flight")
        && heartbeat_activity_recent;
    let provider_wait = last_three
        .iter()
        .any(|event| event.event == "provider_wait")
        && heartbeat_activity_recent;
    let has_recent_heartbeat = heartbeat_activity_recent;
    let has_recent_tool_activity = msg_at.is_some_and(|at| {
        activity_delta_seconds(now, at) < threshold_seconds as f64
            && last_msg
                .is_some_and(|message| python_string_or_empty(message.get("type")) != "tool_use")
    });

    let state = if tool_in_flight || recent_tool_in_flight_heartbeat {
        "tool_in_flight"
    } else if provider_wait {
        "provider_wait"
    } else if has_recent_heartbeat || has_recent_tool_activity {
        "active"
    } else if last_activity_at
        .is_some_and(|at| activity_delta_seconds(now, at) >= threshold_seconds as f64)
    {
        "idle"
    } else {
        "unknown"
    };

    let session_progress = session_progress.and_then(Value::as_object);
    json!({
        "state": state,
        "threshold_seconds": threshold_seconds,
        "last_phase": last_hb.map(|event| event.phase.as_str()),
        "last_event": last_hb.map_or_else(
            || last_msg.map(|message| python_string(message.get("type"))),
            |event| Some(event.event.clone()),
        ),
        "last_detail": last_hb.and_then(|event| event.detail.as_deref()),
        "last_tool": last_hb.map_or_else(
            || last_msg.map(|message| python_string(message.get("tool"))),
            |event| event.tool.clone(),
        ),
        "last_activity_at": last_activity_at.map(format_dt),
        "elapsed_seconds": elapsed_seconds.map(|seconds| (seconds * 100.0).round_ties_even() / 100.0),
        "heartbeat_count": heartbeats.len(),
        "tool_in_flight": tool_in_flight,
        "provider_wait": provider_wait,
        "session_path": last_hb.and_then(|event| event.session_path.as_deref()).filter(|path| !path.is_empty()),
        "has_user_progress": python_truthy(session_progress.and_then(|progress| progress.get("has_user_progress"))),
        "has_assistant_progress": python_truthy(session_progress.and_then(|progress| progress.get("has_assistant_progress"))),
    })
}

/// Return whether the supplied activity analysis classifies as idle.
pub fn is_watchdog_idle(
    heartbeats: &[HeartbeatEvent],
    run_messages: &[Value],
    session_progress: Option<&Value>,
    now: DateTime<Utc>,
    threshold_seconds: i64,
) -> bool {
    analyze_watchdog_activity(
        heartbeats,
        run_messages,
        session_progress,
        now,
        threshold_seconds,
    )["state"]
        == "idle"
}

/// Build the compact watchdog context used in failure feedback.
pub fn build_watchdog_failure_detail(
    heartbeats: &[HeartbeatEvent],
    run_messages: &[Value],
    session_path: Option<&str>,
    session_progress: Option<&Value>,
    now: DateTime<Utc>,
    threshold_seconds: i64,
) -> Value {
    let analysis = analyze_watchdog_activity(
        heartbeats,
        run_messages,
        session_progress,
        now,
        threshold_seconds,
    );
    let resolved_session_path = session_path
        .filter(|path| !path.is_empty())
        .or_else(|| analysis["session_path"].as_str());
    json!({
        "last_phase": analysis["last_phase"],
        "last_event": analysis["last_event"],
        "last_tool": analysis["last_tool"],
        "last_detail": analysis["last_detail"],
        "elapsed_seconds": analysis["elapsed_seconds"],
        "threshold_seconds": threshold_seconds,
        "session_path": resolved_session_path,
        "watchdog_state": analysis["state"],
        "heartbeat_count": analysis["heartbeat_count"],
    })
}

struct AnalysisRequest {
    heartbeats: Vec<HeartbeatEvent>,
    run_messages: Vec<Value>,
    session_progress: Option<Value>,
    now: DateTime<Utc>,
    threshold_seconds: i64,
}

/// Dispatch one stdio JSON request.
///
/// Analysis operations require an explicit `now` field. Supported operations
/// are `parse_dt`, `heartbeat_event_from_dict`, `heartbeat_event_from_line`,
/// `event_round_trip`, `parse_heartbeat_lines`, `activity_helpers`,
/// `analyze_watchdog_activity`, `is_watchdog_idle`, and
/// `build_watchdog_failure_detail`.
pub fn handle_request(request: Value) -> Result<Value, OpsError> {
    let object = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be a JSON object"))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("analyze_watchdog_activity");
    match operation {
        "parse_dt" => {
            let value = object.get("timestamp");
            if !python_truthy(value) {
                return Ok(Value::Null);
            }
            let text = python_string(value);
            Ok(parse_dt(Some(&text)).map_or(Value::Null, |date| Value::String(format_dt(date))))
        }
        "heartbeat_event_from_dict" => Ok(object
            .get("event")
            .and_then(HeartbeatEvent::from_dict)
            .map_or(Value::Null, |event| event.to_dict())),
        "heartbeat_event_from_line" => Ok(object
            .get("line")
            .and_then(Value::as_str)
            .and_then(HeartbeatEvent::from_line)
            .map_or(Value::Null, |event| event.to_dict())),
        "event_round_trip" => {
            let event = object
                .get("event")
                .and_then(HeartbeatEvent::from_dict)
                .ok_or_else(|| {
                    OpsError::invalid_request("event must be a valid heartbeat object")
                })?;
            let line = event.to_line();
            let parsed = HeartbeatEvent::from_line(&line)
                .ok_or_else(|| OpsError::failed("serialized event did not parse"))?;
            Ok(json!({ "event": event.to_dict(), "line": line, "parsed": parsed.to_dict() }))
        }
        "parse_heartbeat_lines" => {
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| OpsError::invalid_request("text must be a string"))?;
            Ok(events_to_json(&parse_heartbeat_lines(Some(text))))
        }
        "activity_helpers" => {
            let heartbeats = parse_request_heartbeats(object)?;
            let run_messages = parse_run_messages(object)?;
            Ok(json!({
                "latest_heartbeat": latest_heartbeat(&heartbeats).map(HeartbeatEvent::to_dict),
                "heartbeat_activity_at": heartbeat_activity_at(&heartbeats).map(format_dt),
                "run_message_activity_at": run_message_activity_at(&run_messages).map(format_dt),
            }))
        }
        "analyze_watchdog_activity" | "is_watchdog_idle" | "build_watchdog_failure_detail" => {
            let analysis_request = parse_analysis_request(object)?;
            match operation {
                "analyze_watchdog_activity" => Ok(analyze_watchdog_activity(
                    &analysis_request.heartbeats,
                    &analysis_request.run_messages,
                    analysis_request.session_progress.as_ref(),
                    analysis_request.now,
                    analysis_request.threshold_seconds,
                )),
                "is_watchdog_idle" => Ok(Value::Bool(is_watchdog_idle(
                    &analysis_request.heartbeats,
                    &analysis_request.run_messages,
                    analysis_request.session_progress.as_ref(),
                    analysis_request.now,
                    analysis_request.threshold_seconds,
                ))),
                _ => Ok(build_watchdog_failure_detail(
                    &analysis_request.heartbeats,
                    &analysis_request.run_messages,
                    object.get("session_path").and_then(Value::as_str),
                    analysis_request.session_progress.as_ref(),
                    analysis_request.now,
                    analysis_request.threshold_seconds,
                )),
            }
        }
        other => Err(OpsError::invalid_request(format!(
            "unknown operation: {other}"
        ))),
    }
}

fn parse_analysis_request(object: &Map<String, Value>) -> Result<AnalysisRequest, OpsError> {
    let heartbeats = parse_request_heartbeats(object)?;
    let run_messages = parse_run_messages(object)?;
    let session_progress = object
        .get("session_progress")
        .filter(|value| !value.is_null())
        .cloned();
    if session_progress
        .as_ref()
        .is_some_and(|value| !value.is_object())
    {
        return Err(OpsError::invalid_request(
            "session_progress must be an object",
        ));
    }
    let now_text = object
        .get("now")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("now must be an explicit ISO timestamp"))?;
    let now = parse_dt(Some(now_text))
        .ok_or_else(|| OpsError::invalid_request("now must be a valid ISO timestamp"))?;
    let threshold = match object.get("threshold_seconds") {
        None => DEFAULT_WATCHDOG_IDLE_THRESHOLD_SECONDS,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| OpsError::invalid_request("threshold_seconds must be an integer"))?,
    };
    Ok(AnalysisRequest {
        heartbeats,
        run_messages,
        session_progress,
        now,
        threshold_seconds: threshold,
    })
}

fn parse_request_heartbeats(object: &Map<String, Value>) -> Result<Vec<HeartbeatEvent>, OpsError> {
    let mut events = Vec::new();
    if let Some(lines) = object.get("heartbeat_lines") {
        match lines {
            Value::String(text) => events.extend(parse_heartbeat_lines(Some(text))),
            Value::Array(lines) => {
                for line in lines {
                    if let Some(line) = line.as_str() {
                        if let Some(event) = HeartbeatEvent::from_line(line) {
                            events.push(event);
                        }
                    }
                }
            }
            Value::Null => {}
            _ => {
                return Err(OpsError::invalid_request(
                    "heartbeat_lines must be text or an array",
                ))
            }
        }
    }
    if let Some(heartbeats) = object.get("heartbeats") {
        let heartbeats = heartbeats
            .as_array()
            .ok_or_else(|| OpsError::invalid_request("heartbeats must be an array"))?;
        for heartbeat in heartbeats {
            let event = match heartbeat {
                Value::String(line) => HeartbeatEvent::from_line(line),
                value => HeartbeatEvent::from_dict(value),
            };
            if let Some(event) = event {
                events.push(event);
            }
        }
    }
    Ok(events)
}

fn parse_run_messages(object: &Map<String, Value>) -> Result<Vec<Value>, OpsError> {
    match object.get("run_messages") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(messages)) => Ok(messages.clone()),
        Some(_) => Err(OpsError::invalid_request("run_messages must be an array")),
    }
}

fn events_to_json(events: &[HeartbeatEvent]) -> Value {
    Value::Array(events.iter().map(HeartbeatEvent::to_dict).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn testdata() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
    }

    fn sample_event(event: &str, ts: &str) -> HeartbeatEvent {
        HeartbeatEvent {
            event: event.to_owned(),
            phase: "fixture".to_owned(),
            controller: "fixture-controller".to_owned(),
            ts: ts.to_owned(),
            detail: None,
            tool: None,
            session_path: None,
        }
    }

    #[test]
    fn python_generated_parity_fixtures_match() {
        let count = agentmesh_ops_kit::check_parity(&testdata(), handle_request)
            .expect("Python-generated fixtures must match");
        assert!(
            count >= 10,
            "expected the full parity fixture set, got {count}"
        );
    }

    #[test]
    fn accepted_event_and_activity_sets_are_pinned() {
        assert_eq!(
            HEARTBEAT_EVENT_TYPES,
            &[
                "phase_start",
                "tool_in_flight",
                "provider_wait",
                "progress",
                "terminal_result"
            ]
        );
        assert_eq!(
            ACTIVITY_HEARTBEAT_EVENTS,
            &["phase_start", "tool_in_flight", "provider_wait", "progress"]
        );
        assert!(!ACTIVITY_HEARTBEAT_EVENTS.contains(&"terminal_result"));
    }

    #[test]
    fn latest_event_and_provider_tool_flags_only_use_last_three_events() {
        let now = parse_dt(Some("2026-07-13T09:00:00Z")).unwrap();
        let heartbeats = vec![
            sample_event("tool_in_flight", "2026-07-13T08:59:00Z"),
            sample_event("provider_wait", "2026-07-13T08:59:10Z"),
            sample_event("progress", "2026-07-13T08:59:20Z"),
            sample_event("progress", "2026-07-13T08:59:30Z"),
            sample_event("progress", "2026-07-13T08:59:40Z"),
        ];
        let result = analyze_watchdog_activity(&heartbeats, &[], None, now, 900);
        assert_eq!(result["state"], "active");
        assert_eq!(result["provider_wait"], false);
        assert_eq!(result["tool_in_flight"], false);
    }

    #[test]
    fn future_activity_clamps_elapsed_without_becoming_idle() {
        let now = parse_dt(Some("2026-07-13T09:00:00Z")).unwrap();
        let heartbeats = vec![sample_event("progress", "2026-07-13T09:00:30Z")];
        let result = analyze_watchdog_activity(&heartbeats, &[], None, now, 900);
        assert_eq!(result["state"], "active");
        assert_eq!(result["elapsed_seconds"], 0.0);
    }

    #[test]
    fn threshold_boundary_is_idle_and_elapsed_is_clamped() {
        let now = parse_dt(Some("2026-07-13T09:00:00Z")).unwrap();
        let exact = vec![sample_event("phase_start", "2026-07-13T08:45:00Z")];
        let result = analyze_watchdog_activity(&exact, &[], None, now, 900);
        assert_eq!(result["state"], "idle");
        assert_eq!(result["elapsed_seconds"], 900.0);
    }

    #[test]
    fn offset_and_invalid_timestamps_are_handled_without_a_clock() {
        assert_eq!(
            parse_dt(Some("2026-07-13T09:00:00+02:00")).map(format_dt),
            Some("2026-07-13T07:00:00+00:00".to_owned())
        );
        assert_eq!(
            parse_dt(Some("2026-07-13T09:00:00+01:00:30")).map(format_dt),
            Some("2026-07-13T07:59:30+00:00".to_owned())
        );
        assert_eq!(
            parse_dt(Some("2026-07-13T09:00:00+01:00:30.5")).map(format_dt),
            Some("2026-07-13T07:59:29.500000+00:00".to_owned())
        );
        assert_eq!(
            parse_dt(Some("2026-07-13T09:00:00+010030")).map(format_dt),
            Some("2026-07-13T07:59:30+00:00".to_owned())
        );
        assert!(parse_dt(Some("2026-07-13T09:00:00+25:00")).is_none());
        assert!(parse_dt(Some("2026-07-13T09:00:00+01:00:60")).is_none());
        assert!(parse_dt(None).is_none());
    }

    #[test]
    fn analysis_requires_an_explicit_now() {
        let error = handle_request(json!({"operation": "analyze_watchdog_activity"}))
            .expect_err("implicit wall-clock fallback must not be accepted");
        assert_eq!(error.kind, agentmesh_ops_kit::ErrorKind::InvalidRequest);
    }
}
