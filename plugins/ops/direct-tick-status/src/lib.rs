//! Tick health calculation over caller-supplied snapshots. No collection or I/O.

use agentmesh_ops_kit::OpsError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use time::macros::{format_description, offset};
use time::{Duration, OffsetDateTime, PrimitiveDateTime, UtcOffset};

pub const JST: UtcOffset = offset!(+9);
pub const ATTENTION: [&str; 3] = ["failing", "stale", "missing"];

#[derive(Deserialize)]
pub struct Spec {
    key: String,
    task: String,
    interval: u32,
    limit: u32,
    hours: Vec<u8>,
}

#[derive(Deserialize)]
struct Request {
    spec: Spec,
    rows: Vec<Map<String, Value>>,
    task: Option<Map<String, Value>>,
    now: String,
}

/// Parse scheduler-style ISO timestamps, treating a missing offset as JST.
/// Python `datetime` truncates fractional seconds to six digits before comparing.
pub fn parse_stamp(value: &str) -> Option<OffsetDateTime> {
    let (date, clock) = value.split_at_checked(10)?;
    let date = time::Date::parse(date, format_description!("[year]-[month]-[day]")).ok()?;
    if date.year() < 1 {
        return None;
    }
    if clock.is_empty() {
        return Some(date.midnight().assume_offset(JST));
    }
    let clock = clock.strip_prefix(['T', 't', ' '])?;
    let (clock, zone) = match clock.find(['+', '-', 'Z']) {
        Some(i) => (&clock[..i], Some(&clock[i..])),
        None => (clock, None),
    };
    let local = PrimitiveDateTime::new(date, parse_clock(clock)?);
    match zone {
        None => Some(local.assume_offset(JST)),
        Some("Z") => Some(local.assume_utc()),
        Some(zone) => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let zone = parse_clock(&zone[1..])?;
            let (h, m, s, fraction) = zone.as_hms_micro();
            let seconds = i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s);
            let micros = if seconds == 0 {
                0
            } else {
                seconds * 1_000_000 + i64::from(fraction)
            };
            local
                .assume_utc()
                .checked_sub(Duration::microseconds(sign * micros))
        }
    }
}

fn parse_clock(clock: &str) -> Option<time::Time> {
    let (clock, fraction) = clock.split_once(['.', ',']).unwrap_or((clock, "0"));
    fn pair(bytes: &[u8]) -> Option<u8> {
        match bytes {
            [a @ b'0'..=b'9', b @ b'0'..=b'9'] => Some((a - b'0') * 10 + (b - b'0')),
            _ => None,
        }
    }
    let (h, m, s) = match clock.as_bytes() {
        [h @ .., b':', a, b, b':', c, d] => (pair(h)?, pair(&[*a, *b])?, pair(&[*c, *d])?),
        [h @ .., b':', a, b] => (pair(h)?, pair(&[*a, *b])?, 0),
        b if b.len() == 6 => (pair(&b[..2])?, pair(&b[2..4])?, pair(&b[4..])?),
        b if b.len() == 4 => (pair(&b[..2])?, pair(&b[2..])?, 0),
        b => (pair(b)?, 0, 0),
    };
    time::Time::from_hms_micro(h, m, s, fraction_micros(fraction)?).ok()
}

fn fraction_micros(fraction: &str) -> Option<u32> {
    if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits = fraction.len().min(6);
    Some(fraction[..digits].parse::<u32>().ok()? * 10_u32.pow((6 - digits) as u32))
}

fn jst_minutes(stamp: OffsetDateTime) -> String {
    let stamp = stamp.to_offset(JST);
    format!(
        "{}T{:02}:{:02}+09:00",
        stamp.date(),
        stamp.hour(),
        stamp.minute()
    )
}

/// Return the live tick and whether it has exceeded its limit. A start equal
/// to the latest finish is not evidence of a new tick.
pub fn in_flight(
    spec: &Spec,
    last_finish: Option<OffsetDateTime>,
    task: &Map<String, Value>,
    now: OffsetDateTime,
) -> Option<Value> {
    if task.get("state")?.as_str()? != "Running" {
        return None;
    }
    let started = parse_stamp(task.get("last_run")?.as_str()?)?;
    if last_finish.is_some_and(|finish| started <= finish) {
        return None;
    }
    let elapsed = now - started;
    let minutes = elapsed.whole_microseconds() as f64 / 1_000_000.0 / 60.0;
    // Formatting rounds the binary float to one decimal, as Python round does.
    let rounded = format!("{minutes:.1}").parse::<f64>().ok()?;
    Some(json!({
        "started_at": jst_minutes(started),
        "minutes": rounded,
        "over_limit": elapsed > Duration::minutes(i64::from(spec.limit)),
    }))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|n| n != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn label(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn ok(row: &Map<String, Value>) -> bool {
    row.get("ok") == Some(&Value::Bool(true))
}

/// Assess rows in the given order; the final row is the newest finish.
pub fn assess(
    spec: &Spec,
    rows: &[Map<String, Value>],
    task: Option<&Map<String, Value>>,
    now: OffsetDateTime,
) -> Result<Value, OpsError> {
    let last_row = rows.last();
    let last = last_row
        .map(|row| {
            row.get("finished_at")
                .and_then(Value::as_str)
                .and_then(parse_stamp)
                .ok_or_else(|| {
                    OpsError::invalid_request("newest row must have a valid finished_at")
                })
        })
        .transpose()?;
    let mut by_health = std::collections::BTreeMap::new();
    for row in rows {
        let health = row.get("health").filter(|value| truthy(value)).map_or_else(
            || {
                if ok(row) {
                    "ok".into()
                } else {
                    "failed".into()
                }
            },
            label,
        );
        *by_health.entry(health).or_insert(0_usize) += 1;
    }
    let streak = rows.iter().rev().take_while(|row| !ok(row)).count();
    let live = task.and_then(|task| in_flight(spec, last, task, now));
    let within_hours = spec.hours.contains(&now.to_offset(JST).hour());
    let late =
        last.is_none_or(|last| now - last > Duration::minutes(2 * i64::from(spec.interval) + 5));
    let verdict = if task.is_none() {
        "missing"
    } else if streak >= 3 {
        "failing"
    } else if within_hours
        && live
            .as_ref()
            .is_some_and(|info| info["over_limit"] == false)
    {
        "running"
    } else if within_hours && late {
        "stale"
    } else if streak > 0 {
        "degraded"
    } else {
        "healthy"
    };
    let mut out = json!({
        "controller": spec.key, "task": spec.task, "ticks": rows.len(), "limit": spec.limit,
        "by_health": by_health, "last_tick": last.map(jst_minutes),
        "verdict": verdict, "consecutive_failures": streak,
    });
    if let Some(task) = task {
        for (key, source) in [
            ("task_state", "state"),
            ("last_task_result", "last_result"),
            ("last_task_run", "last_run"),
            ("next_run", "next_run"),
        ] {
            out[key] = task.get(source).cloned().unwrap_or(Value::Null);
        }
    }
    if let Some(row) = rows.iter().rev().find(|row| !ok(row)) {
        let reason = row
            .get("reason")
            .filter(|value| truthy(value))
            .or_else(|| row.get("health"))
            .unwrap_or(&Value::Null);
        out["last_failure"] = json!({
            "at": row.get("finished_at"),
            "reason": label(reason).chars().take(200).collect::<String>(),
        });
    }
    if let Some(live) = live {
        out["in_flight"] = live;
    }
    if spec.key == "work-queue" {
        if let Some(held) = last_row
            .and_then(|row| row.get("repair_recovery"))
            .and_then(|recovery| recovery.get("held"))
            .filter(|held| truthy(held))
        {
            out["held_sources"] = held.clone();
        }
    }
    if spec.key == "review-sentinel" {
        out["review_wakeups"] = json!(rows
            .iter()
            .filter(|r| r.get("health").and_then(Value::as_str) == Some("review_triggered"))
            .count());
    }
    Ok(out)
}

/// Produce warning findings in controller order; absent detail fields are null.
pub fn findings(controllers: &[Value]) -> Vec<Value> {
    controllers.iter().filter_map(|row| {
        let verdict = row["verdict"].as_str().filter(|v| ATTENTION.contains(v))?;
        let next_action = match verdict {
            "failing" => "Read last_failure and fix that step; the tick retries every interval but repeats the same error.",
            "stale" => "Task Scheduler is not firing (or an in-flight tick outlived its ExecutionTimeLimit): check the task state, the PC session and `register-direct-controller-tasks.ps1`.",
            "missing" => "Register it with `register-direct-controller-tasks.ps1`, or resume its paused Autopilot (never both).",
            _ => unreachable!("not an attention verdict"),
        };
        let detail: Map<String, Value> = ["controller", "task", "last_tick", "consecutive_failures", "last_failure", "task_state", "last_task_result", "last_task_run", "in_flight"]
            .into_iter().map(|key| (key.to_owned(), row[key].clone())).collect();
        Some(json!({ "code": format!("direct_controller_tick_{verdict}"), "severity": "warning",
                     "detail": detail, "count": 1, "next_action": next_action }))
    }).collect()
}

pub fn handle_request(request: Value) -> Result<Value, OpsError> {
    let req: Request = serde_json::from_value(request)
        .map_err(|err| OpsError::invalid_request(format!("invalid assessment request: {err}")))?;
    if req.spec.hours.iter().any(|&hour| hour >= 24) {
        return Err(OpsError::invalid_request(
            "spec.hours must be JST hours between 0 and 23",
        ));
    }
    let now = parse_stamp(&req.now)
        .ok_or_else(|| OpsError::invalid_request("now must be an explicit ISO timestamp"))?;
    let assessment = assess(&req.spec, &req.rows, req.task.as_ref(), now)?;
    let warnings = findings(std::slice::from_ref(&assessment));
    Ok(json!({"assessment": assessment, "findings": warnings}))
}
