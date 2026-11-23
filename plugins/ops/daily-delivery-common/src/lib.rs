//! Pure checks over supplied inputs; this crate never collects evidence or acts on it.

use std::collections::{HashMap, HashSet};

use agentmesh_ops_kit::OpsError;
use serde::Deserialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const SDK_BEHAVIOR_CONTRACT: &str = "pi-sdk-behavior-v2";
pub const SDK_BEHAVIOR_ASSERTIONS: &[(&str, &[&str])] = &[
    ("extension_load", &["extension_registered"]),
    ("happy_path", &["feature_response"]),
    ("error_path", &["handled_error"]),
    (
        "reload_cleanup",
        &[
            "reloaded_extension_registered",
            "reloaded_feature_response",
            "clean_shutdown",
        ],
    ),
    ("host_shell", &["native_shell_boundary"]),
];

#[derive(Debug, Deserialize)]
pub struct Project {
    pub status: Option<String>,
    pub workspace_path: Option<String>,
    pub multica_project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Policy {
    pub allowed_project_statuses: Option<Vec<String>>,
    pub review_delivery_extra_project_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    ManagedDeliveryScope {
        project: Option<Project>,
        policy: Policy,
    },
    DailyTarget {
        remaining: i64,
        deadline: String,
        now: String,
        #[serde(default = "default_minimum")]
        minimum: i64,
    },
    SdkReportReason {
        report: Value,
        versions: HashMap<String, String>,
        cases: Vec<String>,
    },
}

const fn default_minimum() -> i64 {
    10
}

/// Dispatch one request, returning the same JSON scalar as the Python check.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    match serde_json::from_value(request)
        .map_err(|_| OpsError::invalid_request("invalid daily delivery request shape"))?
    {
        Request::ManagedDeliveryScope { project, policy } => {
            Ok(json!(managed_delivery_scope(project.as_ref(), &policy)))
        }
        Request::DailyTarget {
            remaining,
            deadline,
            now,
            minimum,
        } => Ok(json!(daily_target(
            remaining,
            timestamp(&deadline, "deadline")?,
            timestamp(&now, "now")?,
            minimum,
        ))),
        Request::SdkReportReason {
            report,
            versions,
            cases,
        } => Ok(json!(sdk_report_reason(&report, &versions, &cases))),
    }
}

pub fn managed_delivery_scope(project: Option<&Project>, policy: &Policy) -> bool {
    let Some(project) = project else { return false };
    let status = project.status.as_deref().unwrap_or_default();
    if !policy
        .allowed_project_statuses
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|allowed| allowed == status)
    {
        return false;
    }

    let path = project.workspace_path.as_deref().unwrap_or_default();
    let id = project.multica_project_id.as_deref().unwrap_or_default();
    path.replace('\\', "/").contains("/OSS/")
        || (!id.is_empty()
            && policy
                .review_delivery_extra_project_ids
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|extra| extra == id))
}

fn timestamp(value: &str, field: &str) -> Result<OffsetDateTime, OpsError> {
    OffsetDateTime::parse(value, &Rfc3339)
        // Python datetime has neither year zero, leap seconds nor sub-microsecond precision.
        .ok()
        .filter(|ts| ts.year() >= 1 && ts.nanosecond().is_multiple_of(1_000))
        .ok_or_else(|| {
            OpsError::invalid_request(format!(
                "{field} must be an RFC 3339 datetime in Python range and precision"
            ))
        })
}

pub fn daily_target(
    remaining: i64,
    deadline: OffsetDateTime,
    now: OffsetDateTime,
    minimum: i64,
) -> u64 {
    // Match timedelta.total_seconds() and the two ceil operations in Python.
    let micros = (deadline - now).whole_microseconds().max(0) as u64;
    let seconds = int_ratio(micros, 1_000_000);
    let days = (seconds / 86_400.0).ceil().max(1.0) as u32;
    let target = int_ratio(remaining.max(0) as u64, days).ceil() as u64;
    target.max(minimum.max(0) as u64)
}

// Python divides the integers before rounding to binary64. Converting a large
// numerator to f64 first can move the subsequent ceil across a boundary.
fn int_ratio(numerator: u64, denominator: u32) -> f64 {
    if numerator <= (1 << f64::MANTISSA_DIGITS) {
        return numerator as f64 / f64::from(denominator);
    }
    // Keep 64 fractional bits. With these operand bounds the quotient has
    // more than 53 significant bits, so a sticky bit preserves rounding ties
    // when converting it to f64. Scaling by a power of two is exact.
    let scaled = u128::from(numerator) << 64;
    let divisor = u128::from(denominator);
    let quotient = (scaled / divisor) | u128::from(!scaled.is_multiple_of(divisor));
    quotient as f64 / 18_446_744_073_709_551_616.0 // 2^64
}

/// Return the first report rejection reason, or `None` for accepted evidence.
pub fn sdk_report_reason(
    report: &Value,
    versions: &HashMap<String, String>,
    cases: &[String],
) -> Option<String> {
    let reason = |text: &str| Some(text.to_owned());
    if !report.is_object() || !number_eq(&report["schema_version"], 1.0) {
        return reason("sdk_report_schema_missing");
    }
    let same_versions = report["sdk_versions"].as_object().is_some_and(|actual| {
        actual.len() == versions.len()
            && versions
                .iter()
                .all(|(key, version)| actual.get(key).and_then(Value::as_str) == Some(version))
    });
    if !same_versions {
        return reason("sdk_report_wrong_versions");
    }
    if !number_eq(&report["unhandled_errors"], 0.0)
        || !number_eq(&report["extension_load_errors"], 0.0)
    {
        return reason("sdk_runtime_errors");
    }
    if report["verification_contract"].as_str() != Some(SDK_BEHAVIOR_CONTRACT) {
        return reason("sdk_report_behavior_contract_missing");
    }
    let Some(rows) = report["cases"].as_array() else {
        return reason("sdk_report_cases_invalid");
    };
    // Validate every name before checking duplicates, including unrequested cases.
    let Some(by_name) = rows
        .iter()
        .map(|row| Some((row.get("name")?.as_str()?, row)))
        .collect::<Option<HashMap<_, _>>>()
    else {
        return reason("sdk_report_cases_invalid");
    };
    if by_name.len() != rows.len() {
        return reason("sdk_report_duplicate_cases");
    }
    for name in cases {
        if !by_name
            .get(name.as_str())
            .is_some_and(|row| verified_case(name, row))
        {
            return Some(format!("sdk_case_unverified:{name}"));
        }
    }
    None
}

fn verified_case(name: &str, row: &Value) -> bool {
    if !number_eq(&row["exit_code"], 0.0)
        || row["passed"].as_bool() != Some(true)
        || !truthy(&row["command"])
        || !truthy(&row["evidence"])
    {
        return false;
    }
    let Some(assertions) = row["assertions"]
        .as_array()
        .filter(|items| !items.is_empty())
    else {
        return false;
    };
    let mut ids = HashSet::new();
    for assertion in assertions {
        if assertion["passed"].as_bool() != Some(true)
            || !truthy(&assertion["expected"])
            || !truthy(&assertion["observed"])
        {
            return false;
        }
        let Some(id) = assertion["id"].as_str().filter(|id| !id.is_empty()) else {
            return false;
        };
        if !ids.insert(id) {
            return false;
        }
    }
    SDK_BEHAVIOR_ASSERTIONS
        .iter()
        .find(|(case, _)| *case == name)
        .is_none_or(|(_, required)| required.iter().all(|id| ids.contains(id)))
}

// Python numeric equality includes bool, unlike the explicit `is True` checks.
fn number_eq(value: &Value, expected: f64) -> bool {
    value
        .as_f64()
        .or_else(|| value.as_bool().map(|flag| if flag { 1.0 } else { 0.0 }))
        == Some(expected)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(_) => !number_eq(value, 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}
