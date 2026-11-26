//! Offline weekly prevention-hint decisions. This does not filter findings or gates.

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};
use time::macros::{format_description, offset};
use time::{Date, OffsetDateTime, PrimitiveDateTime};

pub const JST: time::UtcOffset = offset!(+9);
pub const PINNED_HINT_KEYS: [&str; 2] = ["G4-migration-no-clobber", "G8-ci-workflow-permissions"];
pub const MAX_TASK_HINTS: usize = 5;

pub fn week_id(now: OffsetDateTime) -> Result<String, OpsError> {
    let (year, week, _) = now
        .checked_to_offset(JST)
        .ok_or_else(|| OpsError::invalid_request("now is out of range in JST"))?
        .to_iso_week_date();
    Ok(format!("{year}-W{week:02}"))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64() != Some(0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}

pub fn decide(stats: &Value, pinned: bool) -> &'static str {
    if pinned {
        "protected_hint"
    } else if truthy(&stats["recently_observed_open"]) {
        "active_observed"
    } else if truthy(&stats["findings_7d"]) {
        "active"
    } else if truthy(&stats["findings_30d"]) {
        "cooling"
    } else {
        "reference_only"
    }
}

fn current_schema(snapshot: &Value) -> bool {
    let version = &snapshot["schema_version"];
    version.as_f64() == Some(1.0) || version.as_bool() == Some(true)
}

fn parse_aware(text: &str) -> Option<OffsetDateTime> {
    // Snapshot producers use ISO timestamps with explicit offsets. Accept
    // both the `T` and space separators used in ISO datetime strings.
    let mut iso = text.to_owned();
    if iso.as_bytes().get(10) == Some(&b' ') {
        iso.replace_range(10..11, "T");
    }
    let parsed =
        OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339).ok()?;
    (parsed.second() == iso.get(17..19)?.parse::<u8>().ok()?).then(|| {
        // Python datetimes have microsecond precision and truncate extra digits.
        parsed
            .replace_nanosecond(parsed.microsecond() * 1_000)
            .unwrap()
    })
}

fn stamp(value: &Value) -> Option<OffsetDateTime> {
    let text = value.as_str()?;
    let date_format = format_description!("[year]-[month]-[day]");
    parse_aware(text)
        // Python's evidence parser interprets naïve timestamps as UTC.
        .or_else(|| parse_aware(&format!("{text}Z")))
        .or_else(|| {
            // Legacy YYYY-MM-DD[T ]HH:MM values also omit seconds.
            if text.len() == 16 {
                parse_aware(&format!("{text}:00Z"))
            } else {
                None
            }
        })
        .or_else(|| {
            Date::parse(text, date_format)
                .ok()
                .map(|d| d.midnight().assume_utc())
        })
}

fn isoformat(now: OffsetDateTime) -> String {
    let clock = PrimitiveDateTime::new(now.date(), now.time());
    let mut text = clock
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]"
        ))
        .expect("all clock components are available");
    if now.microsecond() != 0 {
        text.push_str(&format!(".{:06}", now.microsecond()));
    }
    text.push_str(
        &now.offset()
            .format(format_description!(
                "[offset_hour sign:mandatory]:[offset_minute]"
            ))
            .expect("all offset components are available"),
    );
    text
}

fn object(value: &Value) -> Result<&Map<String, Value>, OpsError> {
    value
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("expected an object snapshot"))
}

fn optional_object<'a>(
    parent: &'a Value,
    name: &str,
) -> Result<Option<&'a Map<String, Value>>, OpsError> {
    parent.get(name).map(object).transpose()
}

fn stats_row(mut row: Value, stats: &Value) -> Result<Value, OpsError> {
    row.as_object_mut()
        .expect("row is an object")
        .extend(object(stats)?.clone());
    Ok(row)
}

/// Return the weekly snapshot and whether this call created it.
pub fn curate(
    index: &Value,
    previous: &Value,
    now: OffsetDateTime,
) -> Result<(Value, bool), OpsError> {
    let week = week_id(now)?;
    let old = &previous["weekly_curation"];
    if current_schema(old) && old["week"] == week {
        return Ok((old.clone(), false));
    }

    let lessons = index
        .get("lessons")
        .map(|v| {
            v.as_array()
                .ok_or_else(|| OpsError::invalid_request("lessons must be an array"))
        })
        .transpose()?;
    let mut global_decisions = Map::new();
    let mut project_decisions = Map::new();
    let mut rows = Vec::new();
    let empty_stats = json!({});
    for lesson in lessons.into_iter().flatten() {
        let key = lesson["key"]
            .as_str()
            .ok_or_else(|| OpsError::invalid_request("lesson requires key"))?;
        let repository_count = lesson
            .get("repository_count")
            .ok_or_else(|| OpsError::invalid_request("lesson requires repository_count"))?;
        let pinned =
            PINNED_HINT_KEYS.contains(&key) || lesson["immediate_risk"].as_bool() == Some(true);
        let stats = lesson.get("stats").unwrap_or(&empty_stats);
        let status = decide(stats, pinned);
        global_decisions.insert(key.to_owned(), json!(status));
        rows.push(stats_row(
            json!({
                "scope": "common", "project_key": null, "key": key, "status": status,
                "repository_count": repository_count,
            }),
            stats,
        )?);

        let mut projects: Vec<_> = optional_object(lesson, "project_stats")?
            .into_iter()
            .flatten()
            .collect();
        projects.sort_unstable_by(|a, b| a.0.cmp(b.0));
        for (project, stats) in projects {
            let status = decide(stats, pinned);
            project_decisions
                .entry(project.clone())
                .or_insert_with(|| json!({}))[key] = json!(status);
            rows.push(stats_row(
                json!({
                    "scope": "project", "project_key": project, "key": key, "status": status,
                }),
                stats,
            )?);
        }
    }

    Ok((
        json!({
            "schema_version": 1,
            "week": week,
            "curated_at": isoformat(now),
            "policy": {
                "active_days": 7, "cooling_days": 30, "evidence_days": 90,
                "max_task_hints": MAX_TASK_HINTS, "pinned_hint_keys": PINNED_HINT_KEYS,
            },
            "global_decisions": global_decisions,
            "project_decisions": project_decisions,
            "measurement": "sampled_finding_counts_only_no_exposure_denominator",
            "rows": rows,
        }),
        true,
    ))
}

pub fn is_visible(curation: &Value, key: &str, stats: &Value, project: Option<&str>) -> bool {
    if !current_schema(curation) {
        return true;
    }
    let table = match project.filter(|p| !p.is_empty()) {
        Some(p) => &curation["project_decisions"][p],
        None => &curation["global_decisions"],
    };
    table[key] != "reference_only"
        || stamp(&stats["last_finding_at"])
            .zip(stamp(&curation["curated_at"]))
            .is_some_and(|(at, curated)| at > curated)
        || truthy(&stats["recently_observed_open"])
}

fn required<'a>(request: &'a Value, name: &str) -> Result<&'a Value, OpsError> {
    request
        .get(name)
        .ok_or_else(|| OpsError::invalid_request(format!("request requires {name}")))
}

fn request_now(request: &Value) -> Result<OffsetDateTime, OpsError> {
    required(request, "now")?
        .as_str()
        .and_then(parse_aware)
        .ok_or_else(|| {
            OpsError::invalid_request("now must be an ISO timestamp with an explicit offset")
        })
}

/// Dispatch a single JSON request to one of the four pure operations.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    match request["op"].as_str() {
        Some("week_id") => Ok(json!(week_id(request_now(&request)?)?)),
        Some("decide") => {
            let stats = required(&request, "stats")?;
            object(stats)?;
            Ok(json!(decide(stats, truthy(&request["pinned"]))))
        }
        Some("curate") => {
            let index = required(&request, "index")?;
            let previous = required(&request, "previous")?;
            object(index)?;
            object(previous)?;
            let (curation, changed) = curate(index, previous, request_now(&request)?)?;
            Ok(json!({"curation": curation, "changed": changed}))
        }
        Some("is_visible") => {
            let curation = required(&request, "curation")?;
            let stats = required(&request, "stats")?;
            object(curation)?;
            object(stats)?;
            let key = required(&request, "key")?
                .as_str()
                .ok_or_else(|| OpsError::invalid_request("key must be a string"))?;
            let project = &request["project"];
            if !project.is_null() && !project.is_string() {
                return Err(OpsError::invalid_request(
                    "project must be a string or null",
                ));
            }
            Ok(json!(is_visible(curation, key, stats, project.as_str())))
        }
        _ => Err(OpsError::invalid_request(
            "expected op: week_id, decide, curate or is_visible",
        )),
    }
}
