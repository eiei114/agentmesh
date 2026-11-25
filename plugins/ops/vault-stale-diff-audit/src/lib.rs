//! Stale-diff classification over caller-supplied data only.

use std::collections::{BTreeMap, BTreeSet};

use agentmesh_ops_kit::OpsError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::macros::format_description;
use time::OffsetDateTime;

pub const DEFAULT_STALE_THRESHOLD_HOURS: f64 = 24.0;

const fn default_threshold() -> f64 {
    DEFAULT_STALE_THRESHOLD_HOURS
}

#[derive(Debug, Deserialize)]
pub struct AuditInput {
    pub vault_root: String,
    pub grouped: BTreeMap<String, Vec<String>>,
    pub excluded_paths: Vec<String>,
    /// A missing key or null value means the path has no readable mtime.
    pub mtimes_unix_us: BTreeMap<String, Option<i64>>,
    pub now_unix_us: i64,
    #[serde(default = "default_threshold")]
    pub threshold_hours: f64,
}

#[derive(Debug, Serialize)]
pub struct StalePath<'a> {
    pub path: &'a str,
    pub age_hours: f64,
}

#[derive(Debug, Serialize)]
pub struct Audit<'a> {
    pub schema_version: u8,
    pub checked_at: String,
    pub vault_root: &'a str,
    pub threshold_hours: f64,
    pub eligible_path_count: usize,
    pub stale: bool,
    pub stale_path_count: usize,
    pub oldest_age_hours: f64,
    pub stale_paths: Vec<StalePath<'a>>,
    pub excluded_paths: &'a [String],
}

pub fn audit_vault_stale_diff(input: &AuditInput) -> Result<Audit<'_>, OpsError> {
    if !input.threshold_hours.is_finite() {
        return Err(OpsError::invalid_request("threshold_hours must be finite"));
    }
    let now = OffsetDateTime::from_unix_timestamp_nanos(i128::from(input.now_unix_us) * 1_000)
        .map_err(|_| OpsError::invalid_request("now_unix_us is outside the datetime range"))?;
    if now.year() < 1 {
        return Err(OpsError::invalid_request("now_unix_us precedes year 1"));
    }
    let date_time = now
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second]"
        ))
        .map_err(|_| OpsError::failed("cannot format checked_at"))?;
    let checked_at = if now.microsecond() == 0 {
        format!("{date_time}+00:00")
    } else {
        format!("{date_time}.{:06}+00:00", now.microsecond())
    };

    let eligible: BTreeSet<_> = input
        .grouped
        .values()
        .flatten()
        .map(String::as_str)
        .collect();
    let mut stale_paths = Vec::new();
    for &path in &eligible {
        let Some(Some(mtime)) = input.mtimes_unix_us.get(path) else {
            continue;
        };
        let delta_us = (i128::from(input.now_unix_us) - i128::from(*mtime)).max(0);
        let age_hours = (delta_us as f64 / 1_000_000.0) / 3600.0;
        if age_hours >= input.threshold_hours {
            // Round the binary float directly, like Python round(age_hours, 2).
            // Scaling and rounding would double-round some values (e.g. 24.005).
            let age_hours = format!("{age_hours:.2}")
                .parse()
                .map_err(|_| OpsError::failed("cannot round age_hours"))?;
            stale_paths.push(StalePath { path, age_hours });
        }
    }
    // Stable sort: ties after rounding keep the lexical order from `eligible`.
    stale_paths.sort_by(|a, b| b.age_hours.total_cmp(&a.age_hours));

    Ok(Audit {
        schema_version: 1,
        checked_at,
        vault_root: &input.vault_root,
        threshold_hours: input.threshold_hours,
        eligible_path_count: eligible.len(),
        stale: !stale_paths.is_empty(),
        stale_path_count: stale_paths.len(),
        oldest_age_hours: stale_paths.first().map_or(0.0, |entry| entry.age_hours),
        stale_paths,
        excluded_paths: &input.excluded_paths,
    })
}

#[derive(Debug, Serialize)]
pub struct Finding<'a> {
    pub code: &'static str,
    pub severity: &'static str,
    pub detail: FindingDetail<'a>,
    pub next_action: &'static str,
}

#[derive(Debug, Serialize)]
pub struct FindingDetail<'a> {
    pub threshold_hours: f64,
    pub stale_path_count: usize,
    pub oldest_age_hours: f64,
    pub paths: Vec<&'a str>,
}

pub fn workflow_health_finding<'a>(audit: &Audit<'a>) -> Option<Finding<'a>> {
    audit.stale.then(|| Finding {
        code: "vault_uncommitted_stale_24h",
        severity: "warning",
        detail: FindingDetail {
            threshold_hours: audit.threshold_hours,
            stale_path_count: audit.stale_path_count,
            oldest_age_hours: audit.oldest_age_hours,
            paths: audit.stale_paths.iter().take(10).map(|item| item.path).collect(),
        },
        next_action: "Run vault-split-commit.py --apply --push or record an explicit skip reason in handoff.",
    })
}

#[derive(Serialize)]
struct ResultDocument<'a> {
    audit: Audit<'a>,
    finding: Option<Finding<'a>>,
}

pub fn handle(request: Value) -> Result<Value, OpsError> {
    let input = serde_json::from_value(request)
        .map_err(|_| OpsError::invalid_request("invalid stale vault diff audit input"))?;
    let audit = audit_vault_stale_diff(&input)?;
    let finding = workflow_health_finding(&audit);
    serde_json::to_value(ResultDocument { audit, finding })
        .map_err(|_| OpsError::failed("cannot serialize stale vault diff audit result"))
}
